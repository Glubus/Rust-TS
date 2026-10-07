use super::super::declarations::is_identifier;

pub(super) fn identifier(name: &str) -> String {
    if is_identifier(name) {
        name.to_owned()
    } else {
        String::from("sdk")
    }
}

pub(super) fn property_name(name: &str) -> String {
    if is_identifier(name) && !is_reserved_word(name) {
        name.to_owned()
    } else {
        format!("{name:?}")
    }
}

pub(super) fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn is_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}
