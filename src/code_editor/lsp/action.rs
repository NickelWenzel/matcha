//! What a server offers to do about a diagnostic or a selection.

use super::{Diagnostic, workspace};

/// Something a language server offers to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAction {
    /// What to call it where a person can read it.
    pub title: String,
    /// What sort of action it is, such as `quickfix` or `refactor.extract`.
    ///
    /// Kinds nest, and the nesting is by dot rather than by prefix: an action
    /// of kind `refactor.extract` answers a request for `refactor`, and one of
    /// kind `refactory` does not. Use [`is_kind`](Self::is_kind) rather than
    /// comparing the strings.
    pub kind: Option<String>,
    /// The problems this action addresses.
    ///
    /// Send them back as they are to ask the server to fill the action in.
    /// A server that put its own identifiers in a diagnostic's `data` cannot
    /// match one that has lost them.
    pub diagnostics: Vec<Diagnostic>,
    /// The changes it makes. When there is a command as well, these go first.
    pub edit: Option<workspace::Edit>,
    /// A command to run, after `edit` if there is one.
    pub command: Option<Command>,
    /// Whether the server considers this the obvious choice.
    ///
    /// Part of how a client decides what to show first, along with the kind and
    /// whether the action fixes a diagnostic the cursor is on.
    pub is_preferred: bool,
    /// Why the action cannot run now, if it cannot.
    ///
    /// Carried rather than filtered out. Whether to hide such an action or show
    /// it greyed is the application's to decide, and this is the sentence a
    /// person needs to read when it is shown.
    pub disabled: Option<String>,
    /// Whatever the server attached, as the JSON text it arrived as.
    ///
    /// Send it back unchanged to ask the server to resolve an action it sent
    /// without its edits.
    pub data: Option<String>,
}

impl CodeAction {
    /// Whether this action answers a request for `kind`.
    ///
    /// An action answers a request for its own kind and for every kind it nests
    /// under: `refactor.extract.function` answers `refactor.extract` and
    /// `refactor`. The separator is what makes that safe. Comparing prefixes
    /// instead would have `refactor` answered by `refactory`, which is a
    /// different kind entirely.
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind.as_deref().is_some_and(|own| {
            own == kind || (own.starts_with(kind) && own[kind.len()..].starts_with('.'))
        })
    }
}

/// A command a language server can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// What to call it where a person can read it.
    pub title: String,
    /// The identifier to send back to run it.
    pub command: String,
    /// Its arguments, one JSON value each, as the text they arrived as.
    ///
    /// Kept separate rather than as one blob so an application can rebuild the
    /// request without matcha reading any of them.
    pub arguments: Vec<String>,
}

/// One entry in what `textDocument/codeAction` answers with.
///
/// A server may answer with either shape in the same list, so a list of
/// [`CodeAction`] alone has nowhere to put a [`Command`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// An action, which may carry edits, a command, or both.
    Action(CodeAction),
    /// A bare command.
    Command(Command),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(kind: Option<&str>) -> CodeAction {
        CodeAction {
            title: "Extract function".to_owned(),
            kind: kind.map(str::to_owned),
            diagnostics: Vec::new(),
            edit: None,
            command: None,
            is_preferred: false,
            disabled: None,
            data: None,
        }
    }

    #[test]
    fn a_kind_answers_every_kind_it_nests_under() {
        let extract = action(Some("refactor.extract.function"));

        assert!(extract.is_kind("refactor.extract.function"));
        assert!(extract.is_kind("refactor.extract"));
        assert!(extract.is_kind("refactor"));
    }

    #[test]
    fn a_kind_does_not_answer_a_request_it_merely_begins_with() {
        // The separator is the whole rule. Comparing prefixes has `refactory`
        // answer a request for `refactor`, and they are different kinds.
        assert!(!action(Some("refactory")).is_kind("refactor"));
        assert!(!action(Some("refactor")).is_kind("refactor.extract"));
        assert!(!action(None).is_kind("refactor"));
    }

    #[test]
    fn an_offer_holds_either_shape() {
        let offers = [
            Offer::Action(action(Some("quickfix"))),
            Offer::Command(Command {
                title: "Run tests".to_owned(),
                command: "test.run".to_owned(),
                arguments: vec!["{\"all\":true}".to_owned()],
            }),
        ];

        assert!(matches!(offers[0], Offer::Action(_)));
        assert!(matches!(offers[1], Offer::Command(_)));
    }

    #[test]
    fn an_action_that_cannot_run_keeps_the_reason_it_cannot() {
        let mut blocked = action(Some("quickfix"));
        blocked.disabled = Some("the file has errors".to_owned());

        assert_eq!(blocked.disabled.as_deref(), Some("the file has errors"));
        assert!(
            blocked.is_kind("quickfix"),
            "a disabled action is still an action, and hiding it is the \
             application's choice to make"
        );
    }
}
