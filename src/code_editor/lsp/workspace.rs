//! Everything a language server wants changed, across every document.

use std::collections::HashMap;

use super::document;

/// Everything a language server wants changed.
///
/// matcha applies the parts that belong to one buffer, through
/// [`Content::apply`](crate::Content::apply). The rest is the application's:
/// routing edits to the documents they name, creating and deleting files, and
/// asking before a change that says it needs asking about. matcha has no notion
/// of a file and will not grow one.
///
/// The protocol carries this two ways, a map and a list, and says the list wins
/// where a client understands it. That is settled on the way in, so what is
/// here is already one ordered sequence whichever way it arrived.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    // Built by the conversions, which are sibling modules, so they reach the
    // fields rather than a constructor. A constructor would put another name on
    // a public type to do the same job, and would take already-normalized parts
    // without being able to say so.
    pub(crate) steps: Vec<Step>,
    pub(crate) annotations: HashMap<String, Annotation>,
}

/// One thing to do, in the order the server asked for it.
///
/// Edits and file operations share a sequence because the order between them
/// carries meaning: create a file, edit it, rename it, edit it again is an
/// ordinary rename refactor, and it is wrong in any other order.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Edits to one document.
    Document(document::Edit),
    /// A file to create, rename or delete.
    Operation(Operation),
}

impl Edit {
    /// Everything to do, in the order the server asked for it.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Just the edits, for an application with one document open.
    ///
    /// A document can appear more than once, each time against a different
    /// version of itself, which is what a rename refactor looks like. They come
    /// back separately and must stay that way: running them together would
    /// apply edits computed against two different texts as though they were
    /// one, and skip whatever the server wanted done between them.
    pub fn document_edits(&self) -> impl Iterator<Item = &document::Edit> {
        self.steps.iter().filter_map(|step| match step {
            Step::Document(edit) => Some(edit),
            Step::Operation(_) => None,
        })
    }

    /// The annotation an edit belongs to, by the identifier it carries.
    ///
    /// An annotation can say that a change needs confirming. matcha never asks
    /// -- it has no interface to ask with -- so an application that honours one
    /// looks the edits up here and decides before applying them.
    pub fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.annotations.get(id)
    }
}

/// A file to create, rename or delete.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Create a file.
    Create(Create),
    /// Rename or move one.
    Rename(Rename),
    /// Delete one.
    Delete(Delete),
}

/// A file to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Create {
    /// The file, as the server spelled it.
    pub uri: String,
    /// Replace it if it is already there.
    pub overwrite: bool,
    /// Leave it alone if it is already there. `overwrite` wins over this.
    pub ignore_if_exists: bool,
    /// The annotation this belongs to.
    pub annotation_id: Option<String>,
}

/// A file to rename or move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    /// Where it is now.
    pub old_uri: String,
    /// Where it should be.
    pub new_uri: String,
    /// Replace the new name if it is already taken.
    pub overwrite: bool,
    /// Do nothing if the new name is already taken. `overwrite` wins over this.
    pub ignore_if_exists: bool,
    /// The annotation this belongs to.
    pub annotation_id: Option<String>,
}

/// A file to delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delete {
    /// The file, as the server spelled it.
    pub uri: String,
    /// Delete the contents of a folder along with it.
    pub recursive: bool,
    /// Do nothing if it is not there.
    pub ignore_if_not_exists: bool,
    /// The annotation this belongs to.
    pub annotation_id: Option<String>,
}

/// A label a server put on a group of changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// What to call the change where a person can read it.
    pub label: String,
    /// Whether the server wants the change confirmed before it is made.
    pub needs_confirmation: bool,
    /// A longer description of it.
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::lsp::{Change, Position, Range, Replacement};

    fn replacement(text: &str, annotation_id: Option<&str>) -> Change {
        Change::Replace(Replacement {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            new_text: text.to_owned(),
            annotation_id: annotation_id.map(str::to_owned),
        })
    }

    fn document(uri: &str, version: Option<i32>, text: &str) -> Step {
        Step::Document(document::Edit {
            uri: uri.to_owned(),
            version,
            edits: vec![replacement(text, None)],
        })
    }

    /// Create a file, edit it, rename it, edit it again: an ordinary rename
    /// refactor, and wrong in any other order.
    fn refactor() -> Edit {
        Edit {
            steps: vec![
                Step::Operation(Operation::Create(Create {
                    uri: "file:///b.rs".to_owned(),
                    overwrite: false,
                    ignore_if_exists: true,
                    annotation_id: None,
                })),
                document("file:///b.rs", Some(1), "first"),
                Step::Operation(Operation::Rename(Rename {
                    old_uri: "file:///b.rs".to_owned(),
                    new_uri: "file:///c.rs".to_owned(),
                    overwrite: false,
                    ignore_if_exists: false,
                    annotation_id: None,
                })),
                document("file:///c.rs", Some(2), "second"),
            ],
            annotations: HashMap::new(),
        }
    }

    #[test]
    fn edits_and_file_operations_keep_the_order_they_arrived_in() {
        let steps = refactor();
        let steps = steps.steps();

        assert_eq!(steps.len(), 4);
        assert!(matches!(steps[0], Step::Operation(Operation::Create(_))));
        assert!(matches!(steps[1], Step::Document(_)));
        assert!(matches!(steps[2], Step::Operation(Operation::Rename(_))));
        assert!(matches!(steps[3], Step::Document(_)));
    }

    #[test]
    fn a_document_edited_twice_comes_back_twice() {
        let edit = refactor();
        let edits: Vec<_> = edit.document_edits().collect();

        assert_eq!(edits.len(), 2);
        assert_eq!(
            (edits[0].version, edits[1].version),
            (Some(1), Some(2)),
            "each batch was computed against a different version of the file, \
             so running them as one would apply the second to text the first \
             has already changed"
        );
    }

    #[test]
    fn one_document_edited_twice_stays_two_batches() {
        // The same file both times, which is what tempts an implementation to
        // merge them.
        let edit = Edit {
            steps: vec![
                document("file:///a.rs", Some(1), "first"),
                document("file:///a.rs", Some(2), "second"),
            ],
            annotations: HashMap::new(),
        };

        let edits: Vec<_> = edit.document_edits().collect();

        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].uri, edits[1].uri);
        assert_ne!(edits[0].version, edits[1].version);
    }

    #[test]
    fn an_edit_finds_the_annotation_it_names() {
        let edit = Edit {
            steps: vec![Step::Document(document::Edit {
                uri: "file:///a.rs".to_owned(),
                version: None,
                edits: vec![replacement("x", Some("rename"))],
            })],
            annotations: HashMap::from([(
                "rename".to_owned(),
                Annotation {
                    label: "Rename symbol".to_owned(),
                    needs_confirmation: true,
                    description: None,
                },
            )]),
        };

        let found = edit.annotation("rename").expect("the identifier is there");

        assert_eq!(found.label, "Rename symbol");
        assert!(
            found.needs_confirmation,
            "an application that honours this has to be able to read it, or \
             carrying the identifier buys nothing"
        );
        assert_eq!(edit.annotation("nothing named this"), None);
    }
}
