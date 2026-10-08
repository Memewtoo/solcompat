//! Small lexical reader for a documented direct-call subset, not a JS type checker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Word,
    Number,
    String,
    Punct,
    Opaque,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Token<'a> {
    pub kind: Kind,
    pub text: &'a str,
    pub start: usize,
    pub end: usize,
}

pub(super) fn tokens(source: &str) -> Vec<Token<'_>> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes.get(i..i + 2) == Some(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes.get(i..i + 2) == Some(b"/*") {
            i += 2;
            while i + 1 < bytes.len() && &bytes[i..i + 2] != b"*/" {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        let kind = if matches!(b, b'\'' | b'"' | b'`') {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == b {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            if b == b'`' {
                Kind::Opaque
            } else {
                Kind::String
            }
        } else if b == b'/'
            && out.last().is_none_or(|previous: &Token<'_>| {
                matches!(
                    previous.text,
                    "=" | "(" | "," | "[" | ":" | ";" | "return" | "!"
                )
            })
        {
            i += 1;
            let mut class = false;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' => {
                        i = (i + 2).min(bytes.len());
                        continue;
                    }
                    b'[' => class = true,
                    b']' => class = false,
                    b'/' if !class => {
                        i += 1;
                        break;
                    }
                    b'\n' => break,
                    _ => {}
                }
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            Kind::Opaque
        } else if b.is_ascii_alphabetic() || matches!(b, b'_' | b'$') {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$'))
            {
                i += 1;
            }
            Kind::Word
        } else if b.is_ascii_digit() {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'.' | b'_'))
            {
                i += 1;
            }
            Kind::Number
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
            Kind::Punct
        };
        out.push(Token {
            kind,
            text: &source[start..i],
            start,
            end: i,
        });
    }
    out
}

pub(super) fn closing(tokens: &[Token<'_>], open: usize) -> Option<usize> {
    let mut stack = Vec::new();
    for (i, token) in tokens.iter().enumerate().skip(open) {
        if token.kind != Kind::Punct {
            continue;
        }
        match token.text {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop()? != token.text {
                    return None;
                }
                if stack.is_empty() {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn split<'a>(tokens: &'a [Token<'a>]) -> Vec<&'a [Token<'a>]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < tokens.len() {
        if matches!(tokens[i].text, "(" | "[" | "{") && tokens[i].kind == Kind::Punct {
            if let Some(end) = closing(tokens, i) {
                i = end + 1;
                continue;
            } else {
                return vec![tokens];
            }
        }
        if tokens[i].text == "," && tokens[i].kind == Kind::Punct {
            out.push(&tokens[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    if start < tokens.len() {
        out.push(&tokens[start..]);
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Property<'a> {
    Missing,
    Literal(&'a str),
    Unknown,
}
pub(super) fn property<'a>(object: &'a [Token<'a>], name: &str) -> Property<'a> {
    if object.first().is_none_or(|t| t.text != "{") || closing(object, 0) != Some(object.len() - 1)
    {
        return Property::Unknown;
    }
    let mut found = None;
    for field in split(&object[1..object.len() - 1]) {
        let Some(key) = field.first() else {
            continue;
        };
        // Spreads, computed properties, shorthand, getters and duplicate keys are unresolved.
        if field.len() < 3 || field[1].text != ":" || !matches!(key.kind, Kind::Word | Kind::String)
        {
            return Property::Unknown;
        }
        let key_text = if key.kind == Kind::String {
            key.text.get(1..key.text.len() - 1).unwrap_or("")
        } else {
            key.text
        };
        if key_text == name {
            if found.is_some() {
                return Property::Unknown;
            }
            found = Some(
                if field.len() == 3 && matches!(field[2].kind, Kind::String | Kind::Number) {
                    Property::Literal(field[2].text)
                } else {
                    Property::Unknown
                },
            );
        }
    }
    found.unwrap_or(Property::Missing)
}

/// Recognize only imports and unique, unreassigned local constructor bindings.
pub(super) fn rpc_bindings(
    tokens: &[Token<'_>],
    source: &str,
) -> std::collections::BTreeMap<String, String> {
    use std::collections::BTreeMap;
    let mut constructors = BTreeMap::new();
    for i in 0..tokens.len() {
        if tokens[i].text != "import"
            || tokens[i].kind != Kind::Word
            || tokens.get(i + 1).is_none_or(|t| t.text != "{")
        {
            continue;
        }
        let end =
            (i + 1..tokens.len()).find(|&j| tokens[j].text == ";" || tokens[j].text == "from");
        let Some(end) = end else {
            continue;
        };
        if tokens[end].text != "from" {
            continue;
        }
        let Some(module) = tokens.get(end + 1) else {
            continue;
        };
        if !matches!(
            module.text,
            "\"@solana/web3.js\"" | "'@solana/web3.js'" | "\"@solana/kit\"" | "'@solana/kit'"
        ) {
            continue;
        }
        for j in i + 1..end {
            if (tokens[j].text == "Connection"
                && module.text.trim_matches(['\'', '"']) == "@solana/web3.js")
                || (tokens[j].text == "createSolanaRpc"
                    && module.text.trim_matches(['\'', '"']) == "@solana/kit")
            {
                let alias = if tokens.get(j + 1).is_some_and(|t| t.text == "as") {
                    tokens.get(j + 2).map(|t| t.text)
                } else {
                    Some(tokens[j].text)
                };
                if let Some(alias) = alias {
                    constructors.insert(
                        alias.to_owned(),
                        module.text.trim_matches(['\'', '"']).to_owned(),
                    );
                }
            }
        }
    }
    // Shadowed imports cannot establish an SDK identity without scope analysis.
    constructors.retain(|name, _| {
        !tokens.windows(2).any(|pair| {
            (matches!(pair[0].text, "const" | "let" | "var" | "function" | "class")
                && pair[1].text == name)
                || (pair[0].text == name && pair[1].text == ":")
        })
    });
    let mut bindings = BTreeMap::new();
    let mut writes = BTreeMap::<String, usize>::new();
    for i in 0..tokens.len().saturating_sub(1) {
        if tokens[i].kind == Kind::Word
            && tokens[i + 1].text == "="
            && tokens.get(i + 2).is_none_or(|t| t.text != "=")
        {
            *writes.entry(tokens[i].text.into()).or_default() += 1;
        }
        if !matches!(tokens[i].text, "const" | "let") {
            continue;
        }
        let Some(name) = tokens.get(i + 1) else {
            continue;
        };
        if name.kind != Kind::Word || tokens.get(i + 2).is_none_or(|t| t.text != "=") {
            continue;
        }
        let call = i + 3 + usize::from(tokens.get(i + 3).is_some_and(|t| t.text == "new"));
        if tokens
            .get(call)
            .is_some_and(|t| constructors.contains_key(t.text))
            && tokens.get(call + 1).is_some_and(|t| t.text == "(")
        {
            let provider = &constructors[tokens[call].text];
            let is_new = tokens[i + 3].text == "new";
            if is_new != (provider == "@solana/web3.js") {
                continue;
            }
            let Some(end) = closing(tokens, call + 1) else {
                continue;
            };
            let standalone = tokens.get(end + 1).is_none_or(|next| {
                matches!(next.text, ";" | ",")
                    || (next.kind == Kind::Word
                        && source[tokens[end].end..next.start].contains('\n'))
            });
            if standalone {
                bindings.insert(name.text.into(), provider.clone());
            }
        }
    }
    // Conservative file-level shadow detection: no lexical scope or type inference.
    let mut parameters = std::collections::BTreeSet::new();
    for i in 0..tokens.len() {
        if tokens[i].text != "(" {
            continue;
        }
        let Some(end) = closing(tokens, i) else {
            continue;
        };
        let function = tokens
            .get(end + 1)
            .is_some_and(|t| t.text == "=" && tokens.get(end + 2).is_some_and(|t| t.text == ">"))
            || (i >= 2 && tokens[i - 2].text == "function")
            || (i >= 3 && tokens[i - 3].text == "function");
        if function {
            for token in &tokens[i + 1..end] {
                if token.kind == Kind::Word {
                    parameters.insert(token.text.to_owned());
                }
            }
        }
    }
    for window in tokens.windows(3) {
        if window[0].kind == Kind::Word && window[1].text == "=" && window[2].text == ">" {
            parameters.insert(window[0].text.to_owned());
        }
    }
    bindings.retain(|name, _| {
        writes.get(name) == Some(&1)
            && !parameters.contains(name)
            && !tokens.windows(2).any(|pair| {
                // Parameters, annotated rebinding, compound assignment and increments need scope analysis.
                pair[0].text == name && matches!(pair[1].text, ":" | "+" | "-" | "*" | "/")
            })
    });
    bindings
}
