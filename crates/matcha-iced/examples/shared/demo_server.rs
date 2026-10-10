//! A scripted language server for the demo examples.
//!
//! It speaks raw JSON-RPC values on the server end of an `lsp_server::Connection::memory()`
//! pair. `step()` answers every message waiting on the connection and returns without blocking,
//! so the app calls it on the UI thread after every update, natively and in the browser.
//! `initialize` is canned; every other answer is computed from the text the client sent:
//!
//! - diagnostics: an error on each `FIXME`, a warning on each `TODO`;
//! - completion: Rust keywords, types and snippets, and the documents' identifiers, where a
//!   function is a snippet that asks for signature help once accepted. The word at the request
//!   position is left out;
//! - hover: a defined item's declaration and `///` doc, else a line on a keyword or common type;
//! - signature help: the enclosing call's function, with the parameter the caret is in;
//! - definition: the `fn`, `struct`, `enum` or `trait` of that name in any document;
//! - rename: whole-word, in every document, leaving comments and strings alone;
//! - formatting: strip trailing whitespace;
//! - inlay hints: a type hint after `let x = f(…)` for a function some document defines (`Self`
//!   reads as the enclosing `impl`), a parameter hint before each call's first argument unless
//!   the argument starts with the parameter's name, and their tooltips through
//!   `inlayHint/resolve`.
//!
//! Nothing here parses Rust: the answers come from text searches, which is enough for the demo
//! samples. Columns are bytes: the initialize result picks `utf-8` positions.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use matcha_iced::lsp;

/// The capabilities: incremental sync, every provider the bridge speaks, and byte columns.
const INITIALIZE: &str = r#"{
    "capabilities": {
        "positionEncoding": "utf-8",
        "textDocumentSync": { "openClose": true, "change": 2 },
        "completionProvider": { "triggerCharacters": ["."] },
        "signatureHelpProvider": { "triggerCharacters": ["(", ","] },
        "hoverProvider": true,
        "definitionProvider": true,
        "renameProvider": true,
        "documentFormattingProvider": true,
        "inlayHintProvider": { "resolveProvider": true }
    },
    "serverInfo": { "name": "matcha-demo", "version": "0.4.0" }
}"#;

/// Where the scripted standard library's `String` lives. No editor opens it, so a jump to it
/// comes back as `Jump::Unopened`.
const STRING_URI: &str = "file:///demo/std/string.rs";

/// The markers diagnostics flag, with their severity: 1 is an error, 2 a warning.
const MARKERS: [(&str, u8); 2] = [("FIXME", 1), ("TODO", 2)];

/// Keywords completion offers as they are.
const KEYWORDS: &[&str] = &[
    "let", "mut", "pub", "const", "use", "mod", "if", "else", "for", "while", "loop", "break",
    "continue", "return", "as", "where",
];

/// Keywords the documents' identifiers never offer.
const UNOFFERED: &[&str] = &[
    "async", "await", "crate", "dyn", "extern", "false", "in", "move", "ref", "self", "Self",
    "static", "super", "true", "type", "unsafe",
];

/// Types completion offers.
const TYPES: &[&str] = &[
    "u8", "u16", "u32", "u64", "usize", "i32", "f64", "bool", "char", "str", "String", "Vec",
    "Option", "Result",
];

/// Snippet completions: label, body, detail, and documentation.
const SNIPPETS: &[(&str, &str, &str, &str)] = &[
    ("fn", "fn ${1:name}(${2:args}) -> ${3:()} {\n\t$0\n}", "name(args) -> ret", "Define a function."),
    ("struct", "struct ${1:Name} {\n\t$0\n}", "Name { fields }", "Define a record type."),
    ("enum", "enum ${1:Name} {\n\t$0\n}", "Name { variants }", "Define a variant type."),
    ("impl", "impl ${1:Type} {\n\t$0\n}", "Type { … }", "Associate methods with a type."),
    ("trait", "trait ${1:Name} {\n\t$0\n}", "Name { … }", "Define a trait."),
    ("match", "match ${1:expr} {\n\t${2:pattern} => $0,\n}", "expr { arms }", "Branch on a value's shape."),
];

/// Hover lines for keywords and common types, as markdown.
const VOCABULARY: &[(&str, &str)] = &[
    ("fn", "**fn** `name(args) -> ret` — defines a function."),
    ("let", "**let** — bind a value to a name; add `mut` to allow reassignment."),
    ("mut", "**mut** — make a binding or reference mutable."),
    ("const", "**const** — a compile-time constant."),
    ("pub", "**pub** — export an item from its module."),
    ("use", "**use** — bring a path into scope."),
    ("mod", "**mod** — declare a module."),
    ("struct", "**struct** `Name { fields }` — a named record type."),
    ("enum", "**enum** `Name { variants }` — a type that is one of several variants."),
    ("impl", "**impl** — associate methods or a trait with a type."),
    ("trait", "**trait** — a set of methods a type can implement."),
    ("match", "**match** `x { pat => expr, … }` — branch on a value's shape."),
    ("for", "**for** `x in iter { … }` — iterate over an iterator."),
    ("while", "**while** `cond { … }` — loop while the condition holds."),
    ("loop", "**loop** — repeat the body forever, until `break`."),
    ("if", "**if** `cond { … } else { … }` — a conditional."),
    ("else", "**else** — the branch taken when the `if` condition is false."),
    ("return", "**return** — return a value from the enclosing function."),
    ("self", "**self** — the receiver of a method."),
    ("Self", "**Self** — the type the enclosing `impl` block is for."),
    ("as", "**as** — a primitive cast, e.g. `n as f64`."),
    ("Option", "**Option<T>** — either `Some(T)` or `None`."),
    ("Some", "**Some(T)** — an `Option` that holds a value."),
    ("None", "**None** — an `Option` that holds nothing."),
    ("Result", "**Result<T, E>** — either `Ok(T)` or `Err(E)`."),
    ("Vec", "**Vec<T>** — a growable, heap-allocated array."),
    ("VecDeque", "**VecDeque<T>** — a double-ended queue."),
    ("String", "**String** — an owned, growable UTF-8 string."),
    ("u8", "**u8** — an unsigned 8-bit integer."),
    ("u32", "**u32** — an unsigned 32-bit integer."),
    ("usize", "**usize** — a pointer-sized unsigned integer."),
    ("f64", "**f64** — a 64-bit floating-point number."),
    ("bool", "**bool** — either `true` or `false`."),
    ("str", "**str** — a borrowed string slice."),
];

/// The item keywords hover and definition look a name up behind.
const ITEMS: &[&str] = &["fn", "struct", "enum", "trait", "const", "static"];

/// The item keywords F12 jumps to.
const DEFINITIONS: &[&str] = &["fn", "struct", "enum", "trait"];

/// The item keywords a type hint links to.
const TYPE_ITEMS: &[&str] = &["struct", "enum"];

/// The server's state: each document as the client last described it, and its end of the
/// connection.
pub struct Scripted {
    connection: lsp::lsp_server::Connection,
    documents: BTreeMap<String, Document>,
}

struct Document {
    version: i64,
    text: String,
}

/// An item some document defines as `<keyword> <name>`.
struct Item<'a> {
    uri: &'a str,
    text: &'a str,
    keyword: &'static str,
    /// Where the keyword starts.
    at: usize,
    /// Where the name starts.
    name_at: usize,
    name: &'a str,
}

/// A function some document defines as `fn name(params) -> Returns`.
struct Function<'a> {
    item: Item<'a>,
    params: Vec<Parameter<'a>>,
    /// The return type as written, `None` for a function that returns nothing.
    returns: Option<&'a str>,
    /// The type of the enclosing `impl`, which `Self` stands for.
    owner: Option<&'a str>,
}

/// One parameter of a [`Function`].
struct Parameter<'a> {
    /// As written, `name: Type`.
    text: &'a str,
    /// The bound name; empty for a pattern.
    name: &'a str,
    name_at: usize,
    /// Whether this is the `self` receiver, which a method call passes before the parentheses.
    receiver: bool,
}

impl Scripted {
    /// A server on `connection`'s end.
    pub fn new(connection: lsp::lsp_server::Connection) -> Self {
        Self {
            connection,
            documents: BTreeMap::new(),
        }
    }

    /// Answers everything the client has sent so far, and returns whether there was anything.
    /// It never blocks, so it runs on the UI thread in a browser too.
    pub fn step(&mut self) -> bool {
        let mut stepped = false;
        while let Ok(message) = self.connection.receiver.try_recv() {
            stepped = true;
            self.receive(&serde_json::to_value(message).expect("lsp-server messages serialize"));
        }
        stepped
    }

    /// Take one message from the client, and send whatever the server says back.
    fn receive(&mut self, wire: &Value) {
        // A response to a server request: this server never asks, so there's nothing to match.
        let Some(method) = wire.get("method").and_then(Value::as_str) else {
            return;
        };
        let params = &wire["params"];
        match wire.get("id") {
            Some(id) => {
                let result = self.answer(method, params);
                self.push(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
            }
            None => self.notified(method, params),
        }
    }

    /// The text the server holds for `uri`, which is what the client's syncs built.
    #[cfg(test)]
    pub fn text(&self, uri: &str) -> Option<&str> {
        self.documents
            .get(uri)
            .map(|document| document.text.as_str())
    }

    fn answer(&self, method: &str, params: &Value) -> Value {
        match method {
            "initialize" => serde_json::from_str(INITIALIZE).expect("INITIALIZE is valid JSON"),
            "textDocument/completion" => self.completion(params),
            "textDocument/signatureHelp" => self.signature_help(params),
            "textDocument/hover" => self.hover(params),
            "textDocument/definition" => self.definition(params),
            "textDocument/rename" => self.rename(params),
            "textDocument/formatting" => self.format(params),
            "textDocument/inlayHint" => self.inlay_hints(params),
            "inlayHint/resolve" => resolve(params),
            // `shutdown`, and anything unscripted.
            _ => Value::Null,
        }
    }

    fn notified(&mut self, method: &str, params: &Value) {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        match method {
            "textDocument/didOpen" => {
                let document = Document {
                    version: params["textDocument"]["version"]
                        .as_i64()
                        .unwrap_or_default(),
                    text: params["textDocument"]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                };
                self.documents.insert(uri.clone(), document);
                self.publish(&uri);
            }
            "textDocument/didChange" => {
                let Some(document) = self.documents.get_mut(&uri) else {
                    return;
                };
                document.version = params["textDocument"]["version"]
                    .as_i64()
                    .unwrap_or(document.version);
                // Changes apply in order, each against the text the previous one left.
                for change in params["contentChanges"].as_array().into_iter().flatten() {
                    let text = change["text"].as_str().unwrap_or_default();
                    match change.get("range") {
                        Some(range) => {
                            let range = offset(&document.text, &range["start"])
                                ..offset(&document.text, &range["end"]);
                            document.text.replace_range(range, text);
                        }
                        None => document.text = text.to_owned(),
                    }
                }
                self.publish(&uri);
            }
            "textDocument/didClose" => {
                self.documents.remove(&uri);
            }
            // `initialized`, `$/cancelRequest` (every reply is already queued), `exit`.
            _ => {}
        }
    }

    /// Publish an error for every `FIXME` and a warning for every `TODO`, in text order.
    fn publish(&mut self, uri: &str) {
        let Some(document) = self.documents.get(uri) else {
            return;
        };
        let text = document.text.as_str();
        let mut found: Vec<(usize, &str, u8)> = MARKERS
            .iter()
            .flat_map(|&(marker, severity)| {
                text.match_indices(marker)
                    .map(move |(at, _)| (at, marker, severity))
            })
            .collect();
        found.sort_unstable();
        let diagnostics: Vec<Value> = found
            .into_iter()
            .map(|(at, marker, severity)| {
                json!({
                    "range": { "start": position(text, at), "end": position(text, at + marker.len()) },
                    "severity": severity,
                    "source": "demo",
                    "message": format!("unresolved {marker}")
                })
            })
            .collect();
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": uri, "version": document.version, "diagnostics": diagnostics }
        });
        self.push(notification);
    }

    /// The document a request's `textDocument` names.
    fn document(&self, params: &Value) -> Option<&str> {
        let uri = params["textDocument"]["uri"].as_str()?;
        self.documents
            .get(uri)
            .map(|document| document.text.as_str())
    }

    /// The identifier under `params`' position.
    fn word_at(&self, params: &Value) -> Option<&str> {
        let text = self.document(params)?;
        let at = offset(text, &params["position"]);
        let start = text[..at]
            .char_indices()
            .rev()
            .take_while(|&(_, c)| is_word(c))
            .last()
            .map_or(at, |(i, _)| i);
        let end = text[at..]
            .char_indices()
            .find(|&(_, c)| !is_word(c))
            .map_or(text.len(), |(i, _)| at + i);
        (start < end).then(|| &text[start..end])
    }

    /// The first `<keyword> name` in any document, for one of `keywords`.
    fn item(&self, name: &str, keywords: &[&'static str]) -> Option<Item<'_>> {
        self.documents.iter().find_map(|(uri, document)| {
            let text = document.text.as_str();
            keywords
                .iter()
                .filter_map(|&keyword| {
                    occurrences(text, keyword)
                        .map(|at| (at, at + keyword.len() + " ".len()))
                        .find(|&(at, name_at)| {
                            text[at + keyword.len()..].starts_with(' ') && word(text, name_at) == name
                        })
                        .map(|(at, name_at)| Item {
                            uri,
                            text,
                            keyword,
                            at,
                            name_at,
                            name: &text[name_at..name_at + name.len()],
                        })
                })
                .min_by_key(|item| item.at)
        })
    }

    /// Every item any document defines, in document and text order.
    fn items(&self) -> Vec<Item<'_>> {
        let mut found = Vec::new();
        for (uri, document) in &self.documents {
            let text = document.text.as_str();
            let mut items: Vec<Item<'_>> = ITEMS
                .iter()
                .flat_map(|&keyword| {
                    occurrences(text, keyword)
                        .filter(move |&at| text[at + keyword.len()..].starts_with(' '))
                        .map(move |at| {
                            let name_at = at + keyword.len() + " ".len();
                            Item {
                                uri,
                                text,
                                keyword,
                                at,
                                name_at,
                                name: word(text, name_at),
                            }
                        })
                })
                .filter(|item| !item.name.is_empty())
                .collect();
            items.sort_by_key(|item| item.at);
            found.extend(items);
        }
        found
    }

    /// Every function any document defines, the first of each name only.
    fn functions(&self) -> Vec<Function<'_>> {
        let mut names = BTreeSet::new();
        self.items()
            .into_iter()
            .filter(|item| item.keyword == "fn")
            .filter_map(Function::parse)
            .filter(|function| names.insert(function.item.name))
            .collect()
    }

    /// The first function named `name`.
    fn function(&self, name: &str) -> Option<Function<'_>> {
        self.item(name, &["fn"]).and_then(Function::parse)
    }

    /// The keywords, types and snippets, then every identifier in the documents, alphabetically.
    /// The word at the request position is left out, so a finished word closes the popup.
    fn completion(&self, params: &Value) -> Value {
        let typed = self.word_at(params);
        let mut items: Vec<Value> = KEYWORDS
            .iter()
            .map(|keyword| json!({ "label": keyword, "kind": 14 }))
            .collect();
        items.extend(SNIPPETS.iter().map(|(label, body, detail, doc)| {
            json!({
                "label": label,
                "kind": 15,
                "detail": detail,
                "documentation": doc,
                "insertTextFormat": 2,
                "insertText": body
            })
        }));
        items.push(json!({
            "label": "println!",
            "kind": 3,
            "filterText": "println",
            "insertTextFormat": 2,
            "insertText": "println!(\"$1\")"
        }));
        items.extend(TYPES.iter().map(|ty| json!({ "label": ty, "kind": 22 })));
        let offered: BTreeSet<&str> = KEYWORDS
            .iter()
            .chain(TYPES)
            .chain(UNOFFERED)
            .copied()
            .chain(SNIPPETS.iter().map(|snippet| snippet.0))
            .collect();
        let identifiers: BTreeSet<&str> = self
            .documents
            .values()
            .flat_map(|document| identifiers(&document.text))
            .filter(|name| !offered.contains(name) && Some(*name) != typed)
            .collect();
        let defined: BTreeMap<&str, Item<'_>> = self
            .items()
            .into_iter()
            .rev()
            .map(|item| (item.name, item))
            .collect();
        for name in identifiers {
            let item = match defined.get(name).map(|item| item.keyword) {
                Some("fn") => match self.function(name) {
                    Some(function) => function.completion(),
                    None => json!({ "label": name, "kind": 3 }),
                },
                Some("struct") => json!({ "label": name, "kind": 22 }),
                Some("enum") => json!({ "label": name, "kind": 13 }),
                Some("trait") => json!({ "label": name, "kind": 8 }),
                _ => json!({ "label": name, "kind": 6 }),
            };
            items.push(item);
        }
        json!({ "isIncomplete": false, "items": items })
    }

    /// A defined item's declaration line and doc comment, else a keyword's or type's line.
    fn hover(&self, params: &Value) -> Value {
        let Some(word) = self.word_at(params) else {
            return Value::Null;
        };
        let markdown = match self.item(word, ITEMS) {
            Some(item) => {
                let line_start = item.text[..item.at].rfind('\n').map_or(0, |i| i + 1);
                let line_end = item.text[item.at..]
                    .find('\n')
                    .map_or(item.text.len(), |i| item.at + i);
                let declaration = item.text[line_start..line_end]
                    .trim()
                    .trim_end_matches('{')
                    .trim_end();
                let mut markdown = format!("```rust\n{declaration}\n```");
                let doc = doc_comment(item.text, item.at);
                if !doc.is_empty() {
                    markdown.push_str("\n---\n");
                    markdown.push_str(&doc);
                }
                markdown
            }
            None => match VOCABULARY.iter().find(|(key, _)| *key == word) {
                Some((_, line)) => (*line).to_owned(),
                None => return Value::Null,
            },
        };
        json!({ "contents": { "kind": "markdown", "value": markdown } })
    }

    /// The signature of the call around the position, with the parameter the caret is in.
    fn signature_help(&self, params: &Value) -> Value {
        let Some(text) = self.document(params) else {
            return Value::Null;
        };
        let before = &text[..offset(text, &params["position"])];
        let Some((name, commas)) = enclosing_call(before) else {
            return Value::Null;
        };
        let Some(function) = self.function(name) else {
            return Value::Null;
        };
        let (label, offsets) = function.label();
        let mut signature = json!({
            "label": label,
            "parameters": offsets
                .into_iter()
                .map(|range| json!({ "label": range }))
                .collect::<Vec<_>>()
        });
        let doc = doc_comment(function.item.text, function.item.at);
        if !doc.is_empty() {
            signature["documentation"] = json!({ "kind": "markdown", "value": doc });
        }
        let active = commas.min(function.arguments().count().saturating_sub(1));
        json!({ "signatures": [signature], "activeSignature": 0, "activeParameter": active })
    }

    /// The name in the first `fn`, `struct`, `enum` or `trait` of that name in any document.
    fn definition(&self, params: &Value) -> Value {
        let Some(word) = self.word_at(params) else {
            return Value::Null;
        };
        match self.item(word, DEFINITIONS) {
            Some(item) => item.location(),
            None => Value::Null,
        }
    }

    /// Every whole-word occurrence of the identifier, in every document, as versioned
    /// `documentChanges`.
    fn rename(&self, params: &Value) -> Value {
        let (Some(word), Some(new_name)) = (self.word_at(params), params["newName"].as_str())
        else {
            return Value::Null;
        };
        let changes: Vec<Value> = self
            .documents
            .iter()
            .filter_map(|(uri, document)| {
                let edits: Vec<Value> = code_words(&document.text)
                    .into_iter()
                    .filter(|&(_, name)| name == word)
                    .map(|(at, _)| {
                        json!({
                            "range": { "start": position(&document.text, at), "end": position(&document.text, at + word.len()) },
                            "newText": new_name
                        })
                    })
                    .collect();
                (!edits.is_empty()).then(|| json!({ "textDocument": { "uri": uri, "version": document.version }, "edits": edits }))
            })
            .collect();
        json!({ "documentChanges": changes })
    }

    /// Trailing whitespace stripped, sent the way many formatters send it: one edit over the
    /// whole document. The client diffs it down to the lines that changed.
    fn format(&self, params: &Value) -> Value {
        let Some(text) = self.document(params) else {
            return Value::Null;
        };
        let lines: Vec<&str> = text.split('\n').collect();
        let formatted = lines
            .iter()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        if formatted == text {
            return json!([]);
        }
        // The end line is one past the last line, which the client clamps to the end of the
        // document.
        json!([{
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": lines.len(), "character": 0 } },
            "newText": formatted
        }])
    }

    /// The hints for one document. The request's range is ignored: the client drops what falls
    /// outside the span it asked for.
    fn inlay_hints(&self, params: &Value) -> Value {
        let Some(text) = self.document(params) else {
            return Value::Null;
        };
        let functions = self.functions();
        let mut hints = Vec::new();
        for function in &functions {
            let Some(parameter) = function.arguments().next() else {
                continue;
            };
            for open in calls(text, function.item.name) {
                let argument = text[open + 1..].trim_start();
                let at = text.len() - argument.len();
                if argument.starts_with(')') || argument.starts_with(parameter.name) {
                    continue;
                }
                hints.push(parameter_hint(text, at, function, parameter));
            }
        }
        for at in occurrences(text, "let") {
            let mut name_at = at + "let ".len();
            if !text[at + "let".len()..].starts_with(' ') {
                continue;
            }
            if text[name_at..].starts_with("mut ") {
                name_at += "mut ".len();
            }
            let name_end = name_at + word(text, name_at).len();
            let line_end = text[name_end..].find('\n').map_or(text.len(), |i| name_end + i);
            let rest = &text[name_end..line_end];
            // `let x: T = …` already states its type.
            if name_end == name_at || !rest.starts_with(" = ") {
                continue;
            }
            let called = functions
                .iter()
                .filter_map(|function| {
                    let returns = function.returns()?;
                    let call = calls(rest, function.item.name).next()?;
                    Some((call, function, returns))
                })
                .min_by_key(|(call, _, _)| *call);
            if let Some((_, function, returns)) = called {
                hints.push(type_hint(text, name_end, function, &returns, self.type_location(&returns)));
            }
        }
        Value::Array(hints)
    }

    /// Where a type hint's type links to: the document's `struct` or `enum` of that name, or the
    /// scripted standard library for `String`.
    fn type_location(&self, ty: &str) -> Option<Value> {
        match self.item(ty, TYPE_ITEMS) {
            Some(item) => Some(item.location()),
            None => (ty == "String").then(|| {
                json!({
                    "uri": STRING_URI,
                    "range": {
                        "start": { "line": 0, "character": 11 },
                        "end": { "line": 0, "character": 17 }
                    }
                })
            }),
        }
    }

    fn push(&mut self, envelope: Value) {
        let message =
            serde_json::from_value(envelope).expect("the script writes valid lsp-server messages");
        // The client end is gone: nobody is left to answer.
        let _ = self.connection.sender.send(message);
    }
}

impl Item<'_> {
    /// The item's name, as an LSP location.
    fn location(&self) -> Value {
        json!({
            "uri": self.uri,
            "range": {
                "start": position(self.text, self.name_at),
                "end": position(self.text, self.name_at + self.name.len())
            }
        })
    }
}

impl<'a> Function<'a> {
    /// The parameters and return type of the `fn` item, `None` when no parameter list follows its
    /// name.
    fn parse(item: Item<'a>) -> Option<Self> {
        let text = item.text;
        let open = item.name_at + item.name.len();
        if !text[open..].starts_with('(') {
            return None;
        }
        let close = closing(text, open)?;
        let params = split(text, open + 1, close)
            .into_iter()
            .map(|(at, param)| Parameter::parse(text, at, param))
            .collect();
        let after = text[close + 1..].trim_start();
        let returns = after.strip_prefix("->").and_then(|rest| {
            let end = rest.find(['{', ';']).unwrap_or(rest.len());
            let ty = &rest[..end];
            let ty = occurrences(ty, "where").next().map_or(ty, |at| &ty[..at]).trim();
            (!ty.is_empty()).then_some(ty)
        });
        let owner = enclosing_impl(text, item.at);
        Some(Self {
            item,
            params,
            returns,
            owner,
        })
    }

    /// The parameters a call passes in its parentheses.
    fn arguments(&self) -> impl Iterator<Item = &Parameter<'a>> {
        self.params.iter().filter(|param| !param.receiver)
    }

    /// The return type with `Self` read as the enclosing `impl`'s type.
    fn returns(&self) -> Option<String> {
        let returns = self.returns?;
        let Some(owner) = self.owner else {
            return Some(returns.to_owned());
        };
        let mut resolved = String::new();
        let mut copied = 0;
        for at in occurrences(returns, "Self") {
            resolved.push_str(&returns[copied..at]);
            resolved.push_str(owner);
            copied = at + "Self".len();
        }
        resolved.push_str(&returns[copied..]);
        Some(resolved)
    }

    /// `name(params) -> Returns`, and the UTF-16 range of each parameter a call passes.
    fn label(&self) -> (String, Vec<[usize; 2]>) {
        let mut label = format!("{}(", self.item.name);
        let mut offsets = Vec::new();
        for (index, param) in self.params.iter().enumerate() {
            if index > 0 {
                label.push_str(", ");
            }
            let start = label.encode_utf16().count();
            label.push_str(param.text);
            if !param.receiver {
                offsets.push([start, label.encode_utf16().count()]);
            }
        }
        label.push(')');
        if let Some(returns) = self.returns {
            label.push_str(" -> ");
            label.push_str(returns);
        }
        (label, offsets)
    }

    /// A snippet that fills in the call's parameters and asks for signature help.
    fn completion(&self) -> Value {
        let placeholders: Vec<String> = self
            .arguments()
            .enumerate()
            .map(|(index, param)| format!("${{{}:{}}}", index + 1, param.name))
            .collect();
        let params: Vec<&str> = self.params.iter().map(|param| param.text).collect();
        let mut detail = format!("fn({})", params.join(", "));
        if let Some(returns) = self.returns {
            detail.push_str(" -> ");
            detail.push_str(returns);
        }
        let mut completion = json!({
            "label": self.item.name,
            "kind": 3,
            "detail": detail,
            "insertTextFormat": 2,
            "insertText": format!("{}({})", self.item.name, placeholders.join(", "))
        });
        let doc = doc_comment(self.item.text, self.item.at);
        if !doc.is_empty() {
            completion["documentation"] = json!({ "kind": "markdown", "value": doc });
        }
        if !placeholders.is_empty() {
            completion["command"] = json!({
                "title": "Signature help",
                "command": "editor.action.triggerParameterHints"
            });
        }
        completion
    }
}

impl<'a> Parameter<'a> {
    /// The parameter written as `param` at byte `at` of `text`.
    fn parse(text: &'a str, at: usize, param: &'a str) -> Self {
        let mut name_at = at;
        let mut rest = param;
        if let Some(stripped) = rest.strip_prefix('&') {
            rest = stripped.trim_start();
            // A lifetime, `&'a self`.
            if let Some(lifetime) = rest.strip_prefix('\'') {
                rest = lifetime[word(lifetime, 0).len()..].trim_start();
            }
        }
        if let Some(stripped) = rest.strip_prefix("mut ") {
            rest = stripped.trim_start();
        }
        name_at += param.len() - rest.len();
        let name = word(text, name_at);
        Self {
            text: param,
            name,
            name_at,
            receiver: name == "self",
        }
    }
}

/// `param:` before a call's first argument; the name links to the parameter.
fn parameter_hint(text: &str, at: usize, function: &Function<'_>, parameter: &Parameter<'_>) -> Value {
    let location = json!({
        "uri": function.item.uri,
        "range": {
            "start": position(function.item.text, parameter.name_at),
            "end": position(function.item.text, parameter.name_at + parameter.name.len())
        }
    });
    json!({
        "position": position(text, at),
        "kind": 2,
        "label": [
            { "value": parameter.name, "location": location },
            { "value": ":" }
        ],
        "paddingRight": true,
        "data": {
            "tooltip": format!("The `{}` parameter of `{}`.", parameter.name, function.item.name)
        }
    })
}

/// `: Returns` after a `let` name, insertable; the type links to `location` when it has one.
fn type_hint(
    text: &str,
    at: usize,
    function: &Function<'_>,
    returns: &str,
    location: Option<Value>,
) -> Value {
    let mut ty = json!({ "value": returns });
    if let Some(location) = location {
        ty["location"] = location;
    }
    json!({
        "position": position(text, at),
        "kind": 1,
        "label": [{ "value": ": " }, ty],
        "textEdits": [{
            "range": { "start": position(text, at), "end": position(text, at) },
            "newText": format!(": {returns}")
        }],
        "data": { "tooltip": format!("What `{}` returns.", function.item.name) }
    })
}

/// `inlayHint/resolve`: the hint, with the tooltip its `data` carries. A real server would look
/// the tooltip up; the script keeps it in `data` so the round trip stays visible.
fn resolve(params: &Value) -> Value {
    let mut hint = params.clone();
    hint["tooltip"] = json!({ "kind": "markdown", "value": params["data"]["tooltip"] });
    hint
}

/// The `///` lines directly above the line holding byte `at`, attributes skipped, without their
/// markers.
fn doc_comment(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let mut lines = Vec::new();
    for line in text[..line_start].lines().rev() {
        let line = line.trim();
        if let Some(doc) = line.strip_prefix("///") {
            lines.push(doc.strip_prefix(' ').unwrap_or(doc));
        } else if !line.starts_with("#[") {
            break;
        }
    }
    lines.reverse();
    lines.join("\n")
}

/// The type of the `impl` block around byte `at`: `X` for `impl X`, `impl<T> X<T>` and
/// `impl Trait for X`.
fn enclosing_impl(text: &str, at: usize) -> Option<&str> {
    let found: Vec<usize> = occurrences(&text[..at], "impl").collect();
    found.into_iter().rev().find_map(|start| {
        let brace = start + text[start..at].find('{')?;
        let mut depth = 0i32;
        for c in text[brace..at].chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        if depth <= 0 {
            return None;
        }
        let mut header = text[start + "impl".len()..brace].trim_start();
        if header.starts_with('<') {
            header = header[closing(header, 0)? + 1..].trim_start();
        }
        if let Some(target) = occurrences(header, "for").last() {
            header = header[target + "for".len()..].trim_start();
        }
        let name = word(header, 0);
        (!name.is_empty()).then_some(name)
    })
}

/// The innermost unclosed call in `before`: the callee's name and the number of top-level commas
/// after its `(`, which is the parameter the caret is in.
fn enclosing_call(before: &str) -> Option<(&str, usize)> {
    let mut depth = 0usize;
    let mut commas = 0;
    for (i, c) in before.char_indices().rev() {
        match c {
            ')' | ']' => depth += 1,
            '(' | '[' if depth > 0 => depth -= 1,
            '(' => {
                let start = before[..i]
                    .char_indices()
                    .rev()
                    .take_while(|&(_, c)| is_word(c))
                    .last()
                    .map_or(i, |(j, _)| j);
                let name = &before[start..i];
                return (!name.is_empty()).then_some((name, commas));
            }
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    None
}

/// The `(` of each call to `name` in `text`, a definition's `fn name(` aside.
fn calls<'a>(text: &'a str, name: &'a str) -> impl Iterator<Item = usize> + 'a {
    occurrences(text, name)
        .filter(move |&at| !text[..at].ends_with("fn ") && text[at + name.len()..].starts_with('('))
        .map(move |at| at + name.len())
}

/// The byte of the bracket that closes the one at `open`, counting `()`, `[]`, `{}` and `<>`.
fn closing(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut previous = ' ';
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            // The `>` of `->` closes nothing.
            '>' if previous == '-' => {}
            ')' | ']' | '}' | '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
        previous = c;
    }
    None
}

/// `text[start..end]` split at its top-level commas, each part trimmed and paired with the byte
/// it starts at; empty parts are dropped.
fn split(text: &str, start: usize, end: usize) -> Vec<(usize, &str)> {
    let mut commas = Vec::new();
    let mut depth = 0usize;
    let mut previous = ' ';
    for (i, c) in text[start..end].char_indices() {
        match c {
            '(' | '[' | '{' | '<' => depth += 1,
            '>' if previous == '-' => {}
            ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => commas.push(start + i),
            _ => {}
        }
        previous = c;
    }
    let froms = std::iter::once(start).chain(commas.iter().map(|comma| comma + 1));
    let tos = commas.iter().copied().chain(std::iter::once(end));
    froms
        .zip(tos)
        .filter_map(|(from, to)| {
            let part = text[from..to].trim();
            (!part.is_empty()).then(|| (to - text[from..to].trim_start().len(), part))
        })
        .collect()
}

/// The identifiers in `text` outside comments and string literals, macro names aside.
fn identifiers(text: &str) -> Vec<&str> {
    code_words(text)
        .into_iter()
        .filter(|&(at, name)| {
            let after = &text[at + name.len()..];
            !(after.starts_with('!') && !after.starts_with("!="))
        })
        .map(|(_, name)| name)
        .collect()
}

/// Every word outside comments and string literals, with its offset. Numbers are left out.
fn code_words(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(c) = text[at..].chars().next() {
        let rest = &text[at..];
        if rest.starts_with("//") {
            at += rest.find('\n').unwrap_or(rest.len());
        } else if c == '"' {
            let bytes = rest.as_bytes();
            let mut i = 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            at = (at + i + 1).min(text.len());
        } else if is_word(c) {
            let name = word(text, at);
            if !c.is_numeric() {
                found.push((at, name));
            }
            at += name.len();
        } else {
            at += c.len_utf8();
        }
    }
    found
}

/// The identifier starting at byte `at`, empty when none does.
fn word(text: &str, at: usize) -> &str {
    let end = text[at..]
        .find(|c: char| !is_word(c))
        .map_or(text.len(), |i| at + i);
    &text[at..end]
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Byte offsets of `word` in `text` where it stands as a whole identifier.
fn occurrences<'a>(text: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(word)
        .map(|(at, _)| at)
        .filter(move |&at| {
            let before = text[..at].chars().next_back().is_some_and(is_word);
            let after = text[at + word.len()..].chars().next().is_some_and(is_word);
            !before && !after
        })
}

/// The position of byte `at`, with byte columns.
fn position(text: &str, at: usize) -> Value {
    let line = text[..at].matches('\n').count();
    let character = at - text[..at].rfind('\n').map_or(0, |i| i + 1);
    json!({ "line": line, "character": character })
}

/// The byte offset of `position`, clamped to its line and to the end of the text.
fn offset(text: &str, position: &Value) -> usize {
    let line = position["line"].as_u64().unwrap_or_default() as usize;
    let character = position["character"].as_u64().unwrap_or_default() as usize;
    let start: usize = text.split_inclusive('\n').take(line).map(str::len).sum();
    let rest = &text[start..];
    start + character.min(rest.find('\n').unwrap_or(rest.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES_URI: &str = "file:///demo/samples.rs";
    const GREET_URI: &str = "file:///demo/greet.rs";

    /// A sample shaped like scratch's: a struct, an `impl` with a constructor returning `Self`, a
    /// `&mut self` method, and a `main` that calls both.
    const SAMPLES: &str = r#"use std::collections::VecDeque;

/// The most recent readings, oldest first.
pub struct Samples {
    values: VecDeque<f64>,
    capacity: usize,
}

impl Samples {
    /// Create an empty window that keeps at most `capacity` readings.
    pub fn new(capacity: usize) -> Self {
        Samples {
            values: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Record one reading.
    pub fn push(&mut self, reading: f64) {
        self.values.push_back(reading);
    }
}

fn main() {
    let mut window = Samples::new(4);
    for reading in [21.5, 22.0] {
        window.push(reading);
    }
}
"#;

    const GREET: &str = "/// Builds a greeting for `name`.\npub fn greet(name: &str) -> String {\n    format!(\"hello, {name}\")\n}\n";

    /// A server and the client end of its connection, driven by hand.
    struct Wire {
        client: lsp::lsp_server::Connection,
        server: Scripted,
        next_id: i64,
    }

    impl Wire {
        fn new() -> Self {
            let (client, server) = lsp::lsp_server::Connection::memory();
            Self {
                client,
                server: Scripted::new(server),
                next_id: 0,
            }
        }

        fn send(&mut self, message: Value) -> Vec<Value> {
            let message = serde_json::from_value(message).expect("a valid lsp-server message");
            self.client.sender.send(message).expect("the server end is open");
            self.server.step();
            let mut received = Vec::new();
            while let Ok(message) = self.client.receiver.try_recv() {
                received.push(serde_json::to_value(message).expect("messages serialize"));
            }
            received
        }

        /// Open `text` at `uri`, and return the diagnostics the server published for it.
        fn open(&mut self, uri: &str, text: &str) -> Vec<Value> {
            let received = self.send(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": uri, "languageId": "rust", "version": 1, "text": text } }
            }));
            let [published] = received.as_slice() else {
                panic!("one publish per open: {received:?}");
            };
            published["params"]["diagnostics"]
                .as_array()
                .expect("a diagnostics array")
                .clone()
        }

        fn request(&mut self, method: &str, params: Value) -> Value {
            self.next_id += 1;
            let received = self.send(json!({
                "jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params
            }));
            let [response] = received.as_slice() else {
                panic!("one response per request: {received:?}");
            };
            response["result"].clone()
        }

        /// Ask `method` at the start of `needle` plus `shift` bytes in `uri`'s `text`.
        fn at(&mut self, method: &str, uri: &str, text: &str, needle: &str, shift: usize) -> Value {
            let at = text.find(needle).unwrap_or_else(|| panic!("{needle:?} is in the text")) + shift;
            let params = json!({ "textDocument": { "uri": uri }, "position": position(text, at) });
            self.request(method, params)
        }
    }

    /// A server with both samples open.
    fn opened() -> Wire {
        let mut wire = Wire::new();
        wire.open(SAMPLES_URI, SAMPLES);
        wire.open(GREET_URI, GREET);
        wire
    }

    #[test]
    fn fixme_is_an_error_and_todo_a_warning() {
        let text = "fn main() {   \n    // TODO: later\n    // FIXME: now\n}\n";
        let diagnostics = Wire::new().open(SAMPLES_URI, text);
        let flagged: Vec<(Value, Value, &str)> = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic["range"]["start"].clone(),
                    diagnostic["severity"].clone(),
                    diagnostic["source"].as_str().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            flagged,
            [
                (json!({ "line": 1, "character": 7 }), json!(2), "demo"),
                (json!({ "line": 2, "character": 7 }), json!(1), "demo"),
            ],
            "the TODO warns and the FIXME errs; the trailing whitespace is not flagged",
        );
    }

    #[test]
    fn hover_shows_a_functions_declaration_and_doc() {
        let hover = opened().at("textDocument/hover", SAMPLES_URI, SAMPLES, "push(reading", 0);
        assert_eq!(
            hover["contents"]["value"],
            "```rust\npub fn push(&mut self, reading: f64)\n```\n---\nRecord one reading.",
        );
    }

    #[test]
    fn hover_on_a_keyword_explains_it() {
        let hover = opened().at("textDocument/hover", SAMPLES_URI, SAMPLES, "let mut", 0);
        let value = hover["contents"]["value"].as_str().unwrap_or_default();
        assert!(value.starts_with("**let**"), "{value}");
    }

    #[test]
    fn hover_on_an_unknown_word_is_null() {
        let hover = opened().at("textDocument/hover", SAMPLES_URI, SAMPLES, "window", 0);
        assert_eq!(hover, Value::Null);
    }

    #[test]
    fn completion_offers_document_identifiers_but_not_the_word_being_typed() {
        let completion = opened().at("textDocument/completion", SAMPLES_URI, SAMPLES, "window =", 3);
        let items = completion["items"].as_array().expect("an item list");
        let item = |label: &str| items.iter().find(|item| item["label"] == label);
        let greet = item("greet").expect("a function from another document");
        assert_eq!(greet["kind"], 3);
        assert_eq!(greet["insertText"], "greet(${1:name})");
        assert_eq!(greet["detail"], "fn(name: &str) -> String");
        assert_eq!(greet["command"]["command"], "editor.action.triggerParameterHints");
        assert_eq!(item("Samples").map(|item| item["kind"].clone()), Some(json!(22)));
        assert_eq!(item("reading").map(|item| item["kind"].clone()), Some(json!(6)));
        assert_eq!(item("let").map(|item| item["kind"].clone()), Some(json!(14)));
        assert!(item("window").is_none(), "the word at the caret is left out");
        assert!(item("format").is_none(), "a macro name is left out");
    }

    #[test]
    fn signature_help_skips_the_receiver() {
        let help = opened().at("textDocument/signatureHelp", SAMPLES_URI, SAMPLES, "push(reading", 5);
        let signature = &help["signatures"][0];
        assert_eq!(signature["label"], "push(&mut self, reading: f64)");
        assert_eq!(signature["parameters"], json!([{ "label": [16, 28] }]));
        assert_eq!(help["activeParameter"], 0);
    }

    #[test]
    fn signature_help_for_greet() {
        let mut wire = opened();
        let call = "    greet(\"a\", ";
        let text = format!("{SAMPLES}fn other() {{\n{call}\n}}\n");
        wire.open("file:///demo/other.rs", &text);
        let help = wire.at("textDocument/signatureHelp", "file:///demo/other.rs", &text, call, call.len());
        let signature = &help["signatures"][0];
        assert_eq!(signature["label"], "greet(name: &str) -> String");
        assert_eq!(signature["parameters"], json!([{ "label": [6, 16] }]));
        assert_eq!(signature["documentation"]["value"], "Builds a greeting for `name`.");
        assert_eq!(help["activeParameter"], 0, "a comma past the last parameter is clamped");
    }

    #[test]
    fn the_sample_gets_a_parameter_hint_and_a_type_hint() {
        let mut wire = opened();
        let params = json!({ "textDocument": { "uri": SAMPLES_URI } });
        let hints = wire.request("textDocument/inlayHint", params);
        let shown: Vec<(Value, String)> = hints
            .as_array()
            .expect("a hint list")
            .iter()
            .map(|hint| {
                let label: String = hint["label"]
                    .as_array()
                    .expect("label parts")
                    .iter()
                    .map(|part| part["value"].as_str().unwrap_or_default())
                    .collect();
                (hint["position"].clone(), label)
            })
            .collect();
        let at = |needle: &str, shift: usize| position(SAMPLES, SAMPLES.find(needle).expect("in the sample") + shift);
        assert_eq!(
            shown,
            [
                (at("new(4)", "new(".len()), "capacity:".to_owned()),
                (at("window =", "window".len()), ": Samples".to_owned()),
            ],
            "`push(reading)` already names its parameter",
        );
        let ty = &hints[1]["label"][1]["location"];
        assert_eq!(ty["uri"], SAMPLES_URI, "the type links to its struct");
        assert_eq!(ty["range"]["start"], at("Samples {", 0));
    }

    /// Rename edits the code, not the prose: the doc comment's `window` stays.
    #[test]
    fn rename_leaves_comments_alone() {
        let at = SAMPLES.find("window =").expect("in the sample");
        let params = json!({
            "textDocument": { "uri": SAMPLES_URI }, "position": position(SAMPLES, at), "newName": "recent"
        });
        let renamed = opened().request("textDocument/rename", params);
        let edits = renamed["documentChanges"][0]["edits"].as_array().expect("edits").clone();
        let code: Vec<Value> = SAMPLES.match_indices("window").map(|(at, _)| position(SAMPLES, at)).collect();
        let starts: Vec<Value> = edits.iter().map(|edit| edit["range"]["start"].clone()).collect();
        assert_eq!(starts, code[1..], "every `window` but the one in the doc comment");
    }

    #[test]
    fn definition_finds_the_struct() {
        let definition = opened().at("textDocument/definition", SAMPLES_URI, SAMPLES, "Samples::new", 0);
        assert_eq!(definition["uri"], SAMPLES_URI);
        let name = SAMPLES.find("struct Samples").expect("in the sample") + "struct ".len();
        assert_eq!(definition["range"]["start"], position(SAMPLES, name));
    }
}
