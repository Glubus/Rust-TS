use syn::{Error, LitStr, Result};

#[derive(Clone, Copy)]
pub(super) enum RenameRule {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

impl RenameRule {
    pub(super) fn parse(lit: &LitStr) -> Result<Self> {
        match lit.value().as_str() {
            "lowercase" => Ok(Self::Lower),
            "UPPERCASE" => Ok(Self::Upper),
            "PascalCase" => Ok(Self::Pascal),
            "camelCase" => Ok(Self::Camel),
            "snake_case" => Ok(Self::Snake),
            "SCREAMING_SNAKE_CASE" => Ok(Self::ScreamingSnake),
            "kebab-case" => Ok(Self::Kebab),
            "SCREAMING-KEBAB-CASE" => Ok(Self::ScreamingKebab),
            _ => Err(Error::new_spanned(lit, "unsupported serde rename_all rule")),
        }
    }

    fn apply_to_field(self, value: &str) -> String {
        let words = split_snake_case(value);
        self.apply_words(&words)
    }

    fn apply_to_variant(self, value: &str) -> String {
        let words = split_pascal_case(value);
        self.apply_words(&words)
    }

    fn apply_words(self, words: &[String]) -> String {
        match self {
            Self::Lower => words.join("").to_lowercase(),
            Self::Upper => words.join("").to_uppercase(),
            Self::Pascal => words.iter().map(|word| upper_first(word)).collect(),
            Self::Camel => camel_case(words),
            Self::Snake => words.join("_").to_lowercase(),
            Self::ScreamingSnake => words.join("_").to_uppercase(),
            Self::Kebab => words.join("-").to_lowercase(),
            Self::ScreamingKebab => words.join("-").to_uppercase(),
        }
    }
}

pub(super) fn rename_field(ident: &syn::Ident, rename_all: Option<RenameRule>) -> String {
    let name = ident.to_string();
    match rename_all {
        Some(rule) => rule.apply_to_field(&name),
        None => name,
    }
}

pub(super) fn rename_variant(ident: &syn::Ident, rename_all: Option<RenameRule>) -> String {
    let name = ident.to_string();
    match rename_all {
        Some(rule) => rule.apply_to_variant(&name),
        None => name,
    }
}

fn split_snake_case(value: &str) -> Vec<String> {
    value
        .split('_')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn split_pascal_case(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_was_lowercase_or_digit = false;

    for character in value.chars() {
        if character.is_uppercase() && previous_was_lowercase_or_digit && !current.is_empty() {
            words.push(current.to_lowercase());
            current = String::new();
        }
        previous_was_lowercase_or_digit = character.is_lowercase() || character.is_ascii_digit();
        current.push(character);
    }

    if !current.is_empty() {
        words.push(current.to_lowercase());
    }

    words
}

fn camel_case(words: &[String]) -> String {
    let Some((first, rest)) = words.split_first() else {
        return String::new();
    };

    let mut output = first.to_lowercase();
    for word in rest {
        output.push_str(&upper_first(word));
    }
    output
}

fn upper_first(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut output = first.to_uppercase().collect::<String>();
    output.push_str(chars.as_str());
    output
}
