//! Declarations as written, and Doxygen comments as Markdown.

use clang::Entity;
use clang::token::TokenKind;

/// Declarations longer than this put each parameter on its own line.
const MAX_LINE: usize = 100;

/// Macro bodies and initializers longer than this are cut (`...`).
const MAX_VALUE: usize = 80;

/// Where a declaration's text stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// At a body, a `;`, or after the parameters at a constructor's
    /// initializers (`:`) or a function-try-block.
    Function,
    /// At a body (`{`) or a `;`: classes, enums, namespaces.
    Body,
    /// At a `;` only, bodies shown as `{ ... }`: typedefs of anonymous
    /// records, variables with brace initializers.
    Whole,
}

/// One piece of a declaration's text: a token, and whether whitespace
/// came before it in the source.
struct Piece {
    text: String,
    space: bool,
}

/// An entity's declaration as the source writes it, from its first token
/// to where `stop` says, whitespace and comments collapsed; a `template
/// <...>` head on a line of its own.
pub(crate) fn declaration(entity: &Entity, stop: Stop) -> Option<String> {
    let tokens = entity.get_range()?.tokenize();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut depth = 0usize;
    let mut params_seen = false;
    let mut last_end: Option<u32> = None;
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        let range = token.get_range();
        let start = range.get_start().get_file_location().offset;
        let end = range.get_end().get_file_location().offset;
        let space = last_end.is_some_and(|e| start > e);
        if token.get_kind() == TokenKind::Comment {
            i += 1;
            continue;
        }
        let text = token.get_spelling();
        match text.as_str() {
            "(" | "[" => depth += 1,
            ")" | "]" => {
                depth = depth.saturating_sub(1);
                if depth == 0 && text == ")" {
                    params_seen = true;
                }
            }
            "{" if depth == 0 => {
                if stop != Stop::Whole {
                    break;
                }
                // Skip to the matching `}`.
                let mut braces = 0usize;
                while i < tokens.len() {
                    match tokens[i].get_spelling().as_str() {
                        "{" => braces += 1,
                        "}" => {
                            braces -= 1;
                            if braces == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                pieces.push(Piece {
                    text: "{ ... }".into(),
                    space,
                });
                last_end = tokens
                    .get(i)
                    .map(|t| t.get_range().get_end().get_file_location().offset);
                i += 1;
                continue;
            }
            ";" if depth == 0 => break,
            ":" | "try" if depth == 0 && stop == Stop::Function && params_seen => break,
            _ => {}
        }
        pieces.push(Piece { text, space });
        last_end = Some(end);
        i += 1;
    }
    if pieces.is_empty() {
        return None;
    }
    // A template head ends at the `>` closing its `<`.
    let mut head_end = None;
    if pieces[0].text == "template" {
        let mut angles = 0i32;
        for (j, piece) in pieces.iter().enumerate().skip(1) {
            match piece.text.as_str() {
                "<" => angles += 1,
                ">" => angles -= 1,
                ">>" => angles -= 2,
                _ => {}
            }
            if angles <= 0 && j > 1 {
                head_end = Some(j);
                break;
            }
        }
    }
    let mut out = String::new();
    for (j, piece) in pieces.iter().enumerate() {
        if j > 0 && head_end == Some(j - 1) {
            out.push('\n');
        } else if piece.space && j > 0 {
            out.push(' ');
        }
        out.push_str(&piece.text);
    }
    Some(wrap(&out))
}

/// A function declaration over [`MAX_LINE`] with its parameters one per
/// line.
fn wrap(text: &str) -> String {
    let (head, decl) = match text.rsplit_once('\n') {
        Some((head, decl)) => (Some(head), decl),
        None => (None, text),
    };
    if decl.len() <= MAX_LINE {
        return text.to_owned();
    }
    let Some(open) = decl.find('(') else {
        return text.to_owned();
    };
    // The parameter list's closing parenthesis.
    let mut depth = 0;
    let mut close = None;
    for (i, c) in decl[open..].char_indices() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return text.to_owned();
    };
    let params = split_top(&decl[open + 1..close]);
    if params.len() < 2 {
        return text.to_owned();
    }
    let wrapped = format!(
        "{}(\n    {}\n){}",
        &decl[..open],
        params.join(",\n    "),
        &decl[close + 1..]
    );
    match head {
        Some(head) => format!("{head}\n{wrapped}"),
        None => wrapped,
    }
}

/// `text` split at commas outside brackets.
pub(crate) fn split_top(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(current.trim().to_owned());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}

/// A macro definition as `#define NAME(args) body`, a long body cut.
pub(crate) fn macro_definition(entity: &Entity) -> Option<String> {
    let tokens = entity.get_range()?.tokenize();
    let mut out = String::from("#define ");
    let mut last_end: Option<u32> = None;
    for token in &tokens {
        let range = token.get_range();
        let start = range.get_start().get_file_location().offset;
        if last_end.is_some_and(|e| start > e) {
            out.push(' ');
        }
        out.push_str(&token.get_spelling());
        last_end = Some(range.get_end().get_file_location().offset);
    }
    let limit = "#define ".len() + MAX_LINE;
    if out.chars().count() > limit {
        let cut: String = out.chars().take(limit).collect();
        out = format!("{cut} ...");
    }
    Some(out)
}

/// `value`, cut to [`MAX_VALUE`] characters.
pub(crate) fn cut(value: &str) -> String {
    if value.chars().count() > MAX_VALUE {
        let cut: String = value.chars().take(MAX_VALUE).collect();
        format!("{cut}...")
    } else {
        value.to_owned()
    }
}

/// A comment without its markers (`/**`, `*/`, leading `*`, `///`,
/// `//!`, `///<`), dedented.
pub(crate) fn strip(raw: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in raw.lines() {
        let mut l = line.trim_start();
        let mut block_line = false;
        for prefix in ["/**<", "/*!<", "/**", "/*!", "/*"] {
            if let Some(rest) = l.strip_prefix(prefix) {
                l = rest;
                block_line = true;
                break;
            }
        }
        if !block_line {
            for prefix in ["///<", "//!<", "///", "//!", "//"] {
                if let Some(rest) = l.strip_prefix(prefix) {
                    l = rest;
                    break;
                }
            }
        }
        let l = l.trim_end();
        let l = l.strip_suffix("*/").unwrap_or(l).trim_end();
        // A block comment's continuation lines start with `*`.
        let l = match l.trim_start().strip_prefix('*') {
            Some(rest) if !rest.starts_with('*') => rest,
            _ => l,
        };
        lines.push(l.to_owned());
    }
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| l.get(indent..).unwrap_or(l.trim_start()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Doxygen's markup in a stripped comment made Markdown: `\code` blocks
/// become fenced code, `\c` / `\p` code, `\b` bold, `\e` / `\a` / `\em`
/// emphasis, `\ref name` and `#name` references (`{<domain>:obj}`). The
/// sections (`\param`, `@return`, ...) stay, for the docstring parser.
pub(crate) fn markdown(text: &str, domain: &str, code_language: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let command = trimmed
            .strip_prefix('\\')
            .or_else(|| trimmed.strip_prefix('@'))
            .unwrap_or("");
        if !in_code && (command.starts_with("code") || command.starts_with("verbatim")) {
            let language = command
                .strip_prefix("code{.")
                .and_then(|r| r.split_once('}'))
                .map_or(
                    if command.starts_with("code") {
                        code_language
                    } else {
                        ""
                    },
                    |(l, _)| l,
                );
            out.push(format!("```{language}"));
            in_code = true;
            continue;
        }
        if in_code && (command.starts_with("endcode") || command.starts_with("endverbatim")) {
            out.push("```".into());
            in_code = false;
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
        }
        if in_code || fenced {
            out.push(line.to_owned());
            continue;
        }
        out.push(inline(line, domain));
    }
    if in_code {
        out.push("```".into());
    }
    out.join("\n")
}

/// Doxygen's inline commands in one line.
fn inline(line: &str, domain: &str) -> String {
    let words: Vec<&str> = line.split(' ').collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    let mut in_code = false;
    while i < words.len() {
        let word = words[i];
        // Backtick code spans pass through.
        if word.matches('`').count() % 2 == 1 {
            in_code = !in_code;
        }
        if in_code || word.contains('`') {
            out.push(word.to_owned());
            i += 1;
            continue;
        }
        let command = word.strip_prefix('\\').or_else(|| word.strip_prefix('@'));
        let next = words.get(i + 1).copied();
        match (command, next) {
            (Some("c" | "p"), Some(arg)) if !arg.is_empty() => {
                let (name, rest) = split_punct(arg);
                out.push(format!("`{name}`{rest}"));
                i += 2;
            }
            (Some("b"), Some(arg)) if !arg.is_empty() => {
                let (name, rest) = split_punct(arg);
                out.push(format!("**{name}**{rest}"));
                i += 2;
            }
            (Some("e" | "em" | "a"), Some(arg)) if !arg.is_empty() => {
                let (name, rest) = split_punct(arg);
                out.push(format!("*{name}*{rest}"));
                i += 2;
            }
            (Some("ref"), Some(arg)) if !arg.is_empty() => {
                let (name, rest) = split_punct(arg);
                out.push(format!("{{{domain}:obj}}`{name}`{rest}"));
                i += 2;
            }
            _ => {
                match word.strip_prefix('#') {
                    Some(name)
                        if name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') =>
                    {
                        let (name, rest) = split_punct(name);
                        out.push(format!("{{{domain}:obj}}`{name}`{rest}"));
                    }
                    _ => out.push(word.to_owned()),
                }
                i += 1;
            }
        }
    }
    out.join(" ")
}

/// A word without its trailing punctuation, and that punctuation.
fn split_punct(word: &str) -> (&str, &str) {
    let base = word.trim_end_matches(['.', ',', ';', ':', '!', '?']);
    // `name()` keeps its parentheses; a closing one of a parenthesis
    // around it does not.
    let end = if base.ends_with("()") {
        base.len()
    } else {
        base.trim_end_matches(')').len()
    };
    (&word[..end], &word[end..])
}

/// The doc comment just above line `line` (from 1) of `source`: a block
/// comment (`/**`, `/*!`) ending on the line before, or a run of `///` or
/// `//!` lines. libclang does not attach comments to macros.
pub(crate) fn comment_above(source: &str, line: u32) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut i = (line as usize).checked_sub(2)?;
    let last = lines.get(i)?.trim();
    if last.ends_with("*/") {
        let end = i;
        loop {
            let l = lines.get(i)?.trim_start();
            if l.starts_with("/**") || l.starts_with("/*!") {
                return Some(lines[i..=end].join("\n"));
            }
            if l.starts_with("/*") {
                return None;
            }
            i = i.checked_sub(1)?;
        }
    }
    let is_doc = |l: &str| {
        let l = l.trim_start();
        l.starts_with("///") || l.starts_with("//!")
    };
    if !is_doc(last) {
        return None;
    }
    let end = i;
    while i > 0 && is_doc(lines[i - 1]) {
        i -= 1;
    }
    Some(lines[i..=end].join("\n"))
}

/// The file's documentation: the doc comment holding `@file` (or
/// `\file`), without that line.
pub(crate) fn file_comment(source: &str) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let at = lines.iter().position(|l| {
        let t = l
            .trim_start()
            .trim_start_matches(['/', '*', '!'])
            .trim_start();
        t == "@file" || t == "\\file" || t.starts_with("@file ") || t.starts_with("\\file ")
    })?;
    // The comment around it.
    let mut start = at;
    while start > 0 && !lines[start].contains("/*") && !lines[start].trim_start().starts_with("//")
    {
        start -= 1;
    }
    if lines[start].trim_start().starts_with("//") {
        while start > 0 && lines[start - 1].trim_start().starts_with("//") {
            start -= 1;
        }
    }
    let mut end = at;
    if lines[start].contains("/*") {
        while end < lines.len() && !lines[end].contains("*/") {
            end += 1;
        }
    } else {
        while end + 1 < lines.len() && lines[end + 1].trim_start().starts_with("//") {
            end += 1;
        }
    }
    let block = lines.get(start..=end.min(lines.len().saturating_sub(1)))?;
    let text: Vec<&str> = block
        .iter()
        .enumerate()
        .filter(|(i, _)| start + i != at)
        .map(|(_, l)| *l)
        .collect();
    let stripped = strip(&text.join("\n"));
    (!stripped.trim().is_empty()).then_some(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments() {
        assert_eq!(
            strip("/**\n * One.\n *\n *     Two.\n */"),
            "One.\n\n    Two."
        );
        assert_eq!(strip("/// One.\n/// Two."), "One.\nTwo.");
        assert_eq!(strip("///< Trailing."), "Trailing.");
        assert_eq!(
            markdown(
                "Use \\c foo, \\p bar. and @ref Baz. Call \\c f(). (see \\c g)\nSee #VEC_MAX and \\b bold `\\c x`.\n\\code\nint x;\n\\endcode\n@param x - The x.",
                "c",
                "c"
            ),
            "Use `foo`, `bar`. and {c:obj}`Baz`. Call `f()`. (see `g`)\nSee {c:obj}`VEC_MAX` and **bold** `\\c x`.\n```c\nint x;\n```\n@param x - The x."
        );
        let source = "int a;\n/** Doc\n * more. */\n#define X 1\n/// One\n/// two\n#define Y 2\n";
        assert_eq!(
            comment_above(source, 4).as_deref(),
            Some("/** Doc\n * more. */")
        );
        assert_eq!(
            comment_above(source, 7).as_deref(),
            Some("/// One\n/// two")
        );
        assert_eq!(comment_above(source, 1), None);
        assert_eq!(
            file_comment("/**\n * @file\n * The file.\n */\nint x;\n").as_deref(),
            Some("The file.")
        );
    }

    #[test]
    fn wrapping() {
        let long = "void transform(const shape_t *alpha, const shape_t *beta, double gamma, double delta, int count) const";
        assert_eq!(
            wrap(long),
            "void transform(\n    const shape_t *alpha,\n    const shape_t *beta,\n    double gamma,\n    double delta,\n    int count\n) const"
        );
        let template = format!("template <typename T>\n{long}");
        assert!(wrap(&template).starts_with("template <typename T>\nvoid transform(\n"));
        assert_eq!(split_top("a<b, c> x, int y"), ["a<b, c> x", "int y"]);
    }
}
