//! VBA parser: tokens → [`ModuleAst`].

use std::rc::Rc;

use crate::VbaError;
use crate::ast::*;
use crate::lexer::{Tok, Token, lex};

/// Deepest expression or block nesting accepted.
const MAX_NEST: usize = 200;

pub fn parse_module(src: &str) -> Result<ModuleAst, VbaError> {
    let toks = preprocess(lex(src)?);
    let mut p = Parser { toks, pos: 0, depth: 0, lines: true };
    p.module()
}

/// Parses a statement list (for tests and the immediate window).
pub fn parse_statements(src: &str) -> Result<Vec<Stmt>, VbaError> {
    let toks = preprocess(lex(src)?);
    let mut p = Parser { toks, pos: 0, depth: 0, lines: false };
    let body = p.block(&[])?;
    if p.pos < p.toks.len() {
        return Err(p.err("unexpected statement"));
    }
    Ok(body)
}

/// Conditional compilation: keeps the `#If` branches that apply. `VBA7`, `Win64` and `Mac`
/// (false) are the constants we know; `#Const` adds more.
fn preprocess(toks: Vec<Token>) -> Vec<Token> {
    let mut lines: Vec<Vec<Token>> = vec![vec![]];
    for t in toks {
        let nl = t.tok == Tok::Newline;
        if let Some(l) = lines.last_mut() {
            l.push(t);
        }
        if nl {
            lines.push(vec![]);
        }
    }
    if !lines.iter().any(|l| matches!(l.first(), Some(Token { tok: Tok::Op("#"), .. }))) {
        return lines.into_iter().flatten().collect();
    }
    let mut consts: Vec<(String, bool)> =
        vec![("vba7".into(), true), ("vba6".into(), true), ("win64".into(), true), ("win32".into(), true), ("mac".into(), false)];
    // Stack of (this branch active, some branch already taken, parent active).
    let mut stack: Vec<(bool, bool, bool)> = Vec::new();
    let mut out = Vec::new();
    let eval = |line: &[Token], consts: &[(String, bool)]| -> bool {
        // `#If A [And|Or] [Not] B … Then`: evaluated left to right.
        let mut acc: Option<bool> = None;
        let mut op = "";
        let mut neg = false;
        for t in line.iter().skip(2) {
            match &t.tok {
                Tok::Ident(w) if w.eq_ignore_ascii_case("then") => break,
                Tok::Ident(w) if w.eq_ignore_ascii_case("not") => neg = !neg,
                Tok::Ident(w) if w.eq_ignore_ascii_case("and") => op = "and",
                Tok::Ident(w) if w.eq_ignore_ascii_case("or") => op = "or",
                Tok::Ident(w) => {
                    let v = consts.iter().find(|(n, _)| n.eq_ignore_ascii_case(w)).is_some_and(|(_, v)| *v) != neg;
                    neg = false;
                    acc = Some(match (acc, op) {
                        (None, _) => v,
                        (Some(a), "or") => a || v,
                        (Some(a), _) => a && v,
                    });
                }
                Tok::Int(n) => acc = Some(*n != 0),
                _ => {}
            }
        }
        acc.unwrap_or(false)
    };
    for line in lines {
        let active = stack.last().is_none_or(|s| s.0);
        let is_directive = matches!(line.first(), Some(Token { tok: Tok::Op("#"), .. }));
        if !is_directive {
            if active {
                out.extend(line);
            }
            continue;
        }
        let word = match line.get(1).map(|t| &t.tok) {
            Some(Tok::Ident(w)) => w.to_ascii_lowercase(),
            _ => String::new(),
        };
        match word.as_str() {
            "if" => {
                let v = active && eval(&line, &consts);
                stack.push((v, v, active));
            }
            "elseif" => {
                if let Some(top) = stack.last_mut() {
                    let v = top.2 && !top.1 && eval(&line, &consts);
                    top.0 = v;
                    top.1 |= v;
                }
            }
            "else" => {
                if let Some(top) = stack.last_mut() {
                    top.0 = top.2 && !top.1;
                    top.1 = true;
                }
            }
            "end" => {
                stack.pop();
            }
            "const" if active => {
                if let Some(Tok::Ident(n)) = line.get(2).map(|t| &t.tok) {
                    let v = matches!(line.get(4).map(|t| &t.tok), Some(Tok::Ident(w)) if w.eq_ignore_ascii_case("true"))
                        || matches!(line.get(4).map(|t| &t.tok), Some(Tok::Int(n)) if *n != 0);
                    consts.push((n.clone(), v));
                }
            }
            _ => {}
        }
    }
    out
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: usize,
    /// Put a [`Stmt::Line`] before each statement.
    lines: bool,
}

/// Words that end an argument list written without parentheses or a single-line `If` branch.
fn is_reserved(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "and" | "or" | "not" | "xor" | "eqv" | "imp" | "mod" | "is" | "like" | "then" | "else" | "elseif" | "to" | "step" | "in"
    )
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }
    fn peek_at(&self, k: usize) -> Option<&Tok> {
        self.toks.get(self.pos + k).map(|t| &t.tok)
    }
    fn line(&self) -> u32 {
        self.toks.get(self.pos).or_else(|| self.toks.last()).map_or(0, |t| t.line)
    }
    fn err(&self, msg: impl Into<String>) -> VbaError {
        VbaError::Syntax { line: self.line(), msg: msg.into() }
    }
    fn is_kw_at(&self, k: usize, w: &str) -> bool {
        matches!(self.peek_at(k), Some(Tok::Ident(x)) if x.eq_ignore_ascii_case(w))
    }
    fn is_kw(&self, w: &str) -> bool {
        self.is_kw_at(0, w)
    }
    fn eat_kw(&mut self, w: &str) -> bool {
        if self.is_kw(w) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_kw(&mut self, w: &str) -> Result<(), VbaError> {
        if self.eat_kw(w) { Ok(()) } else { Err(self.err(format!("expected `{w}`"))) }
    }
    fn is_op(&self, o: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(x)) if *x == o)
    }
    fn eat_op(&mut self, o: &str) -> bool {
        if self.is_op(o) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect_op(&mut self, o: &str) -> Result<(), VbaError> {
        if self.eat_op(o) { Ok(()) } else { Err(self.err(format!("expected `{o}`"))) }
    }
    /// End of statement: a newline, `:`, or the end of the tokens.
    fn at_eos(&self) -> bool {
        matches!(self.peek(), None | Some(Tok::Newline) | Some(Tok::Op(":")))
    }
    fn skip_eos(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline) | Some(Tok::Op(":"))) {
            self.pos += 1;
        }
    }
    fn skip_line(&mut self) {
        while !matches!(self.peek(), None | Some(Tok::Newline)) {
            self.pos += 1;
        }
    }
    fn ident(&mut self) -> Result<String, VbaError> {
        match self.peek().cloned() {
            Some(Tok::Ident(s)) | Some(Tok::Bracketed(s)) => {
                self.pos += 1;
                Ok(s)
            }
            _ => Err(self.err("expected a name")),
        }
    }
    fn nest(&mut self) -> Result<(), VbaError> {
        self.depth += 1;
        if self.depth > MAX_NEST { Err(self.err("nested too deeply")) } else { Ok(()) }
    }

    // ------------------------------------------------------------------ module level

    fn module(&mut self) -> Result<ModuleAst, VbaError> {
        let mut m = ModuleAst::default();
        loop {
            self.skip_eos();
            if self.peek().is_none() {
                break;
            }
            let line = self.line();
            if self.is_kw("Attribute")
                || self.is_kw("Implements")
                || self.is_kw("DefInt")
                || self.is_kw("DefLng")
                || self.is_kw("DefStr")
                || self.is_kw("Event")
            {
                self.skip_line();
                continue;
            }
            if self.eat_kw("Option") {
                if self.eat_kw("Base") {
                    if let Some(Tok::Int(n)) = self.peek().cloned() {
                        m.option_base = n;
                    }
                } else if self.eat_kw("Compare") {
                    m.option_compare_text = self.is_kw("Text");
                }
                self.skip_line();
                continue;
            }
            let mut private = false;
            let mut scoped = false;
            loop {
                if self.eat_kw("Private") {
                    private = true;
                    scoped = true;
                } else if self.eat_kw("Public") || self.eat_kw("Global") || self.eat_kw("Friend") {
                    scoped = true;
                } else if self.is_kw("Static") && (self.is_kw_at(1, "Sub") || self.is_kw_at(1, "Function") || self.is_kw_at(1, "Property")) {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.is_kw("Sub") || self.is_kw("Function") || self.is_kw("Property") {
                let proc = self.procedure(private, line)?;
                m.procs.push(proc);
            } else if self.eat_kw("Declare") {
                self.eat_kw("PtrSafe");
                let _ = self.eat_kw("Sub") || self.eat_kw("Function");
                let name = self.ident()?;
                self.expect_kw("Lib")?;
                let lib = match self.peek().cloned() {
                    Some(Tok::Str(s)) => s,
                    _ => String::new(),
                };
                self.skip_line();
                m.declares.push(Declare { name, lib });
            } else if self.eat_kw("Const") {
                for (n, e) in self.const_list()? {
                    m.consts.push((n, e, private));
                }
            } else if self.eat_kw("Type") {
                let name = self.ident()?;
                let mut members = Vec::new();
                loop {
                    self.skip_eos();
                    if self.peek().is_none() {
                        return Err(self.err("`Type` without `End Type`"));
                    }
                    if self.is_kw("End") && self.is_kw_at(1, "Type") {
                        self.pos += 2;
                        break;
                    }
                    members.push(self.var_decl()?);
                    self.skip_line();
                }
                m.types.push((name, members));
            } else if self.eat_kw("Enum") {
                let name = self.ident()?;
                let mut members = Vec::new();
                loop {
                    self.skip_eos();
                    if self.peek().is_none() {
                        return Err(self.err("`Enum` without `End Enum`"));
                    }
                    if self.is_kw("End") && self.is_kw_at(1, "Enum") {
                        self.pos += 2;
                        break;
                    }
                    let n = self.ident()?;
                    let v = if self.eat_op("=") { Some(self.expr()?) } else { None };
                    members.push((n, v));
                    self.skip_line();
                }
                m.enums.push((name, members));
            } else if self.eat_kw("Dim") || scoped || self.eat_kw("Static") {
                self.eat_kw("WithEvents");
                for d in self.decl_list()? {
                    m.vars.push((d, private));
                }
            } else {
                return Err(self.err("expected a declaration or a procedure"));
            }
            if !self.at_eos() {
                return Err(self.err("unexpected text after the declaration"));
            }
        }
        Ok(m)
    }

    fn type_name(&mut self) -> Result<TypeName, VbaError> {
        let n = self.ident()?;
        // `As Excel.Range`, `As MSForms.TextBox`.
        let n = if self.eat_op(".") { self.ident()? } else { n };
        // `As String * 10` (fixed length).
        if self.eat_op("*") {
            let _ = self.primary()?;
        }
        Ok(match n.to_ascii_lowercase().as_str() {
            "variant" | "any" => TypeName::Variant,
            "integer" => TypeName::Integer,
            "long" | "longptr" => TypeName::Long,
            "longlong" => TypeName::LongLong,
            "byte" => TypeName::Byte,
            "single" => TypeName::Single,
            "double" => TypeName::Double,
            "currency" => TypeName::Currency,
            "decimal" => TypeName::Decimal,
            "date" => TypeName::Date,
            "string" => TypeName::String,
            "boolean" => TypeName::Boolean,
            "object" => TypeName::Object,
            _ => TypeName::Named(n),
        })
    }

    fn bounds(&mut self) -> Result<Vec<(Option<Expr>, Expr)>, VbaError> {
        let mut v = Vec::new();
        if self.eat_op(")") {
            return Ok(v);
        }
        loop {
            let a = self.expr()?;
            if self.eat_kw("To") {
                let b = self.expr()?;
                v.push((Some(a), b));
            } else {
                v.push((None, a));
            }
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op(")")?;
        Ok(v)
    }

    fn var_decl(&mut self) -> Result<VarDecl, VbaError> {
        let name = self.ident()?;
        let bounds = if self.eat_op("(") { Some(self.bounds()?) } else { None };
        let mut new = false;
        let ty = if self.eat_kw("As") {
            new = self.eat_kw("New");
            self.type_name()?
        } else {
            TypeName::Variant
        };
        Ok(VarDecl { name, ty, bounds, new })
    }

    fn decl_list(&mut self) -> Result<Vec<VarDecl>, VbaError> {
        let mut v = vec![self.var_decl()?];
        while self.eat_op(",") {
            self.eat_kw("WithEvents");
            v.push(self.var_decl()?);
        }
        Ok(v)
    }

    fn const_list(&mut self) -> Result<Vec<(String, Expr)>, VbaError> {
        let mut v = Vec::new();
        loop {
            let n = self.ident()?;
            if self.eat_kw("As") {
                self.type_name()?;
            }
            self.expect_op("=")?;
            v.push((n, self.expr()?));
            if !self.eat_op(",") {
                break;
            }
        }
        Ok(v)
    }

    fn procedure(&mut self, private: bool, line: u32) -> Result<Proc, VbaError> {
        let (kind, end) = if self.eat_kw("Sub") {
            (ProcKind::Sub, "Sub")
        } else if self.eat_kw("Function") {
            (ProcKind::Function, "Function")
        } else {
            self.expect_kw("Property")?;
            let k = if self.eat_kw("Get") {
                ProcKind::PropertyGet
            } else if self.eat_kw("Let") {
                ProcKind::PropertyLet
            } else {
                self.expect_kw("Set")?;
                ProcKind::PropertySet
            };
            (k, "Property")
        };
        let name = self.ident()?;
        let mut params = Vec::new();
        if self.eat_op("(") && !self.eat_op(")") {
            loop {
                let mut p = Param {
                    name: String::new(),
                    by_val: false,
                    optional: false,
                    default: None,
                    param_array: false,
                    ty: TypeName::Variant,
                    array: false,
                };
                loop {
                    if self.eat_kw("Optional") {
                        p.optional = true;
                    } else if self.eat_kw("ByVal") {
                        p.by_val = true;
                    } else if self.eat_kw("ByRef") {
                    } else if self.eat_kw("ParamArray") {
                        p.param_array = true;
                    } else {
                        break;
                    }
                }
                p.name = self.ident()?;
                if self.eat_op("(") {
                    self.expect_op(")")?;
                    p.array = true;
                }
                if self.eat_kw("As") {
                    p.ty = self.type_name()?;
                }
                if self.eat_op("=") {
                    p.default = Some(self.expr()?);
                }
                params.push(p);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
        }
        let ret = if self.eat_kw("As") {
            let t = self.type_name()?;
            if self.eat_op("(") {
                self.expect_op(")")?;
            }
            t
        } else {
            TypeName::Variant
        };
        let body = self.block(&[&["end", &end.to_ascii_lowercase()]])?;
        if !(self.eat_kw("End") && self.eat_kw(end)) {
            return Err(self.err(format!("`{name}` has no `End {end}`")));
        }
        Ok(Proc { name, kind, private, params, ret, body: Rc::new(body), line })
    }

    // ------------------------------------------------------------------ statements

    /// Whether the statement at the cursor starts with one of `terms` (each a word sequence).
    fn at_term(&self, terms: &[&[&str]]) -> bool {
        terms.iter().any(|t| t.iter().enumerate().all(|(k, w)| self.is_kw_at(k, w)))
    }

    /// Statements until one of `terms` (or the end of input when `terms` is empty).
    fn block(&mut self, terms: &[&[&str]]) -> Result<Vec<Stmt>, VbaError> {
        self.nest()?;
        let mut out = Vec::new();
        loop {
            self.skip_eos();
            if self.peek().is_none() {
                if terms.is_empty() {
                    break;
                }
                self.depth -= 1;
                return Err(self.err(format!("missing `{}`", terms.first().map(|t| t.join(" ")).unwrap_or_default())));
            }
            if !terms.is_empty() && self.at_term(terms) {
                break;
            }
            if self.lines {
                out.push(Stmt::Line(self.line()));
            }
            let st = self.statement()?;
            let label = matches!(st, Stmt::Label(_));
            out.push(st);
            if !label && !self.at_eos() && !self.is_kw("Else") {
                return Err(self.err("unexpected text after the statement"));
            }
        }
        self.depth -= 1;
        Ok(out)
    }

    fn statement(&mut self) -> Result<Stmt, VbaError> {
        // Labels: `Name:` or a line number at the start of a line.
        let line_start = self.pos == 0 || matches!(self.toks.get(self.pos - 1).map(|t| &t.tok), Some(Tok::Newline));
        if line_start {
            match (self.peek().cloned(), self.peek_at(1)) {
                (Some(Tok::Ident(n)), Some(Tok::Op(":"))) if !is_reserved(&n) && !self.is_kw("Else") => {
                    self.pos += 2;
                    return Ok(Stmt::Label(n));
                }
                (Some(Tok::Int(n)), _) => {
                    // A line number: a label, with the line's statement after it.
                    self.pos += 1;
                    self.eat_op(":");
                    return Ok(Stmt::Label(n.to_string()));
                }
                _ => {}
            }
        }
        if self.eat_kw("Dim") || self.eat_kw("Private") || self.eat_kw("Public") {
            return Ok(Stmt::Dim(self.decl_list()?));
        }
        if self.eat_kw("Static") {
            return Ok(Stmt::Static(self.decl_list()?));
        }
        if self.eat_kw("Const") {
            return Ok(Stmt::Const(self.const_list()?));
        }
        if self.eat_kw("ReDim") {
            let preserve = self.eat_kw("Preserve");
            return Ok(Stmt::ReDim { preserve, decls: self.decl_list()? });
        }
        if self.eat_kw("Erase") {
            let mut v = vec![self.expr()?];
            while self.eat_op(",") {
                v.push(self.expr()?);
            }
            return Ok(Stmt::Erase(v));
        }
        if self.eat_kw("Set") {
            let lhs = self.target()?;
            self.expect_op("=")?;
            return Ok(Stmt::Set(lhs, self.expr()?));
        }
        if self.eat_kw("Let") {
            let lhs = self.target()?;
            self.expect_op("=")?;
            return Ok(Stmt::Let(lhs, self.expr()?));
        }
        if self.eat_kw("Call") {
            let t = self.target()?;
            return Ok(match t {
                Expr::Call(f, args) => Stmt::Call(*f, args),
                t => Stmt::Call(t, vec![]),
            });
        }
        if self.is_kw("If") {
            return self.if_stmt();
        }
        if self.eat_kw("For") {
            return self.for_stmt();
        }
        if self.eat_kw("Do") {
            return self.do_stmt();
        }
        if self.eat_kw("While") {
            let c = self.expr()?;
            let body = self.block(&[&["wend"]])?;
            self.expect_kw("Wend")?;
            return Ok(Stmt::While(c, body));
        }
        if self.is_kw("Select") && self.is_kw_at(1, "Case") {
            self.pos += 2;
            return self.select_stmt();
        }
        if self.eat_kw("With") {
            let subject = self.expr()?;
            let body = self.block(&[&["end", "with"]])?;
            self.pos += 2;
            return Ok(Stmt::With(subject, body));
        }
        if self.eat_kw("Exit") {
            let k = if self.eat_kw("Sub") {
                ExitKind::Sub
            } else if self.eat_kw("Function") {
                ExitKind::Function
            } else if self.eat_kw("Property") {
                ExitKind::Property
            } else if self.eat_kw("For") {
                ExitKind::For
            } else if self.eat_kw("Do") {
                ExitKind::Do
            } else {
                return Err(self.err("expected `Sub`, `Function`, `For` or `Do` after `Exit`"));
            };
            return Ok(Stmt::Exit(k));
        }
        if self.eat_kw("GoTo") {
            return Ok(Stmt::GoTo(self.label_name()?));
        }
        if self.is_kw("On") && self.is_kw_at(1, "Error") {
            self.pos += 2;
            if self.eat_kw("Resume") {
                self.expect_kw("Next")?;
                return Ok(Stmt::OnError(OnError::ResumeNext));
            }
            self.expect_kw("GoTo")?;
            if matches!(self.peek(), Some(Tok::Int(0))) {
                self.pos += 1;
                return Ok(Stmt::OnError(OnError::GoToZero));
            }
            if self.eat_op("-") {
                // `On Error GoTo -1` clears the current error.
                self.pos += 1;
                return Ok(Stmt::OnError(OnError::GoToZero));
            }
            return Ok(Stmt::OnError(OnError::GoTo(self.label_name()?)));
        }
        if self.eat_kw("Resume") {
            if self.at_eos() {
                return Ok(Stmt::Resume(None));
            }
            if self.eat_kw("Next") {
                return Ok(Stmt::Resume(Some("next".into())));
            }
            return Ok(Stmt::Resume(Some(self.label_name()?)));
        }
        if self.is_kw("End") && !self.is_kw_at(1, "Sub") && !self.is_kw_at(1, "Function") && !self.is_kw_at(1, "If") {
            self.pos += 1;
            return Ok(Stmt::End);
        }
        // `Attribute Macro1.VB_ProcData.VB_Invoke_Func = …` lines recorded macros carry inside
        // their bodies are the editor's bookkeeping.
        if self.is_kw("Option") || self.is_kw("Stop") || (self.is_kw("Attribute") && matches!(self.peek_at(1), Some(Tok::Ident(_)))) {
            self.skip_line();
            return Ok(Stmt::Nop);
        }
        // `Debug.Print a; b` and other calls; assignments.
        let target = self.target()?;
        if self.eat_op("=") {
            return Ok(Stmt::Let(target, self.expr()?));
        }
        if self.at_eos() || self.is_kw("Else") {
            return Ok(match target {
                Expr::Call(f, args) => Stmt::Call(*f, args),
                t => Stmt::Call(t, vec![]),
            });
        }
        let args = self.args_until_eos()?;
        Ok(Stmt::Call(target, args))
    }

    fn label_name(&mut self) -> Result<String, VbaError> {
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.pos += 1;
                Ok(n.to_string())
            }
            _ => self.ident(),
        }
    }

    fn if_stmt(&mut self) -> Result<Stmt, VbaError> {
        self.expect_kw("If")?;
        let cond = self.expr()?;
        self.expect_kw("Then")?;
        if !matches!(self.peek(), None | Some(Tok::Newline)) {
            // Single-line If: statements separated by `:` up to `Else` or the line end.
            let then = self.inline_stmts()?;
            let otherwise = if self.eat_kw("Else") { self.inline_stmts()? } else { vec![] };
            return Ok(Stmt::If { arms: vec![(cond, then)], otherwise });
        }
        let mut arms = Vec::new();
        let body = self.block(&[&["elseif"], &["else"], &["end", "if"]])?;
        arms.push((cond, body));
        let mut otherwise = vec![];
        loop {
            if self.eat_kw("ElseIf") {
                let c = self.expr()?;
                self.expect_kw("Then")?;
                let b = self.block(&[&["elseif"], &["else"], &["end", "if"]])?;
                arms.push((c, b));
            } else if self.eat_kw("Else") {
                // `Else If …` on one line is an ElseIf.
                otherwise = self.block(&[&["end", "if"]])?;
            } else {
                self.expect_kw("End")?;
                self.expect_kw("If")?;
                break;
            }
        }
        Ok(Stmt::If { arms, otherwise })
    }

    fn inline_stmts(&mut self) -> Result<Vec<Stmt>, VbaError> {
        let mut v = Vec::new();
        loop {
            while self.eat_op(":") {}
            if matches!(self.peek(), None | Some(Tok::Newline)) || self.is_kw("Else") {
                break;
            }
            if self.is_kw("If") {
                // A nested single-line If owns the rest of the line.
                v.push(self.if_stmt()?);
                break;
            }
            v.push(self.statement()?);
        }
        Ok(v)
    }

    fn for_stmt(&mut self) -> Result<Stmt, VbaError> {
        if self.eat_kw("Each") {
            let var = self.target()?;
            self.expect_kw("In")?;
            let items = self.expr()?;
            let body = self.block(&[&["next"]])?;
            self.next_kw()?;
            return Ok(Stmt::ForEach { var, items, body });
        }
        let var = self.target()?;
        self.expect_op("=")?;
        let from = self.expr()?;
        self.expect_kw("To")?;
        let to = self.expr()?;
        let step = if self.eat_kw("Step") { Some(self.expr()?) } else { None };
        let body = self.block(&[&["next"]])?;
        self.next_kw()?;
        Ok(Stmt::For { var, from, to, step, body })
    }

    /// `Next`, `Next i`, or `Next j, i` (which closes the enclosing loop too: left in place as a
    /// `Next i` for the outer loop).
    fn next_kw(&mut self) -> Result<(), VbaError> {
        self.expect_kw("Next")?;
        if matches!(self.peek(), Some(Tok::Ident(_))) && !self.is_kw("Else") {
            self.pos += 1;
            if self.is_op(",") {
                // Rewrite `, i` into `Next i` for the enclosing loop.
                if let Some(t) = self.toks.get_mut(self.pos) {
                    t.tok = Tok::Ident("Next".into());
                }
            }
        }
        Ok(())
    }

    fn do_stmt(&mut self) -> Result<Stmt, VbaError> {
        let pre = if self.eat_kw("While") {
            Some((self.expr()?, false, true))
        } else if self.eat_kw("Until") {
            Some((self.expr()?, true, true))
        } else {
            None
        };
        let body = self.block(&[&["loop"]])?;
        self.expect_kw("Loop")?;
        let post = if self.eat_kw("While") {
            Some((self.expr()?, false, false))
        } else if self.eat_kw("Until") {
            Some((self.expr()?, true, false))
        } else {
            None
        };
        Ok(Stmt::Do { cond: pre.or(post), body })
    }

    fn select_stmt(&mut self) -> Result<Stmt, VbaError> {
        let subject = self.expr()?;
        let mut cases = Vec::new();
        let mut otherwise = None;
        loop {
            self.skip_eos();
            if self.is_kw("End") && self.is_kw_at(1, "Select") {
                self.pos += 2;
                break;
            }
            self.expect_kw("Case")?;
            let terms: &[&[&str]] = &[&["case"], &["end", "select"]];
            if self.eat_kw("Else") {
                otherwise = Some(self.block(terms)?);
                continue;
            }
            let mut tests = Vec::new();
            loop {
                if self.eat_kw("Is") {
                    let op = match self.peek() {
                        Some(Tok::Op("=")) => BinOp::Eq,
                        Some(Tok::Op("<>")) => BinOp::Ne,
                        Some(Tok::Op("<")) => BinOp::Lt,
                        Some(Tok::Op(">")) => BinOp::Gt,
                        Some(Tok::Op("<=")) => BinOp::Le,
                        Some(Tok::Op(">=")) => BinOp::Ge,
                        _ => return Err(self.err("expected a comparison after `Case Is`")),
                    };
                    self.pos += 1;
                    tests.push(CaseTest::Is(op, self.expr()?));
                } else {
                    let a = self.expr()?;
                    if self.eat_kw("To") {
                        tests.push(CaseTest::Range(a, self.expr()?));
                    } else {
                        tests.push(CaseTest::Value(a));
                    }
                }
                if !self.eat_op(",") {
                    break;
                }
            }
            let body = self.block(terms)?;
            cases.push((tests, body));
        }
        Ok(Stmt::Select { subject, cases, otherwise })
    }

    // ------------------------------------------------------------------ expressions

    /// Arguments written without parentheses, to the end of the statement.
    fn args_until_eos(&mut self) -> Result<Vec<Arg>, VbaError> {
        let mut v = Vec::new();
        loop {
            if self.at_eos() || self.is_kw("Else") {
                break;
            }
            if self.is_op(",") || self.is_op(";") {
                self.pos += 1;
                if v.is_empty() {
                    v.push(Arg { name: None, value: Expr::Missing, by_val: false });
                }
                if self.at_eos() || self.is_op(",") {
                    v.push(Arg { name: None, value: Expr::Missing, by_val: false });
                }
                continue;
            }
            v.push(self.arg()?);
            if !(self.is_op(",") || self.is_op(";")) {
                break;
            }
        }
        Ok(v)
    }

    fn arg(&mut self) -> Result<Arg, VbaError> {
        if let (Some(Tok::Ident(n)), Some(Tok::Op(":="))) = (self.peek().cloned(), self.peek_at(1)) {
            self.pos += 2;
            return Ok(Arg { name: Some(n), value: self.expr()?, by_val: false });
        }
        let by_val = self.eat_kw("ByVal");
        Ok(Arg { name: None, value: self.expr()?, by_val })
    }

    /// Arguments in parentheses (the `(` already read).
    fn paren_args(&mut self) -> Result<Vec<Arg>, VbaError> {
        let mut v = Vec::new();
        if self.eat_op(")") {
            return Ok(v);
        }
        loop {
            if self.is_op(",") || self.is_op(")") {
                v.push(Arg { name: None, value: Expr::Missing, by_val: false });
            } else {
                v.push(self.arg()?);
            }
            if self.eat_op(",") {
                continue;
            }
            self.expect_op(")")?;
            break;
        }
        Ok(v)
    }

    /// An assignable or callable target: a name or `.member` with member accesses, calls and
    /// `!` lookups after it.
    fn target(&mut self) -> Result<Expr, VbaError> {
        let base = if self.eat_op(".") {
            Expr::WithMember(self.ident()?)
        } else if self.is_op("(") {
            self.pos += 1;
            let e = self.expr()?;
            self.expect_op(")")?;
            e
        } else {
            Expr::Name(self.ident()?)
        };
        self.postfix(base)
    }

    fn postfix(&mut self, mut e: Expr) -> Result<Expr, VbaError> {
        loop {
            if self.is_op(".") && matches!(self.peek_at(1), Some(Tok::Ident(_)) | Some(Tok::Bracketed(_))) {
                self.pos += 1;
                let m = self.ident()?;
                e = Expr::Member(Box::new(e), m);
            } else if self.is_op("!") && matches!(self.peek_at(1), Some(Tok::Ident(_)) | Some(Tok::Bracketed(_))) {
                self.pos += 1;
                let m = self.ident()?;
                e = Expr::Call(Box::new(e), vec![Arg { name: None, value: Expr::Str(m.into()), by_val: false }]);
            } else if self.is_op("(") {
                self.pos += 1;
                self.nest()?;
                let args = self.paren_args()?;
                self.depth -= 1;
                e = Expr::Call(Box::new(e), args);
            } else {
                return Ok(e);
            }
        }
    }

    pub fn expr(&mut self) -> Result<Expr, VbaError> {
        self.nest()?;
        let e = self.imp();
        self.depth -= 1;
        e
    }

    fn binary_level(&mut self, ops: &[(&str, BinOp)], next: fn(&mut Self) -> Result<Expr, VbaError>) -> Result<Expr, VbaError> {
        let mut l = next(self)?;
        'outer: loop {
            for (w, op) in ops {
                let hit = match self.peek() {
                    Some(Tok::Op(o)) => o == w,
                    Some(Tok::Ident(x)) => x.eq_ignore_ascii_case(w),
                    _ => false,
                };
                if hit {
                    self.pos += 1;
                    let r = next(self)?;
                    l = Expr::Binary(*op, Box::new(l), Box::new(r));
                    continue 'outer;
                }
            }
            return Ok(l);
        }
    }

    fn imp(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("Imp", BinOp::Imp)], Self::eqv)
    }
    fn eqv(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("Eqv", BinOp::Eqv)], Self::xor)
    }
    fn xor(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("Xor", BinOp::Xor)], Self::or)
    }
    fn or(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("Or", BinOp::Or)], Self::and)
    }
    fn and(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("And", BinOp::And)], Self::not)
    }
    fn not(&mut self) -> Result<Expr, VbaError> {
        if self.eat_kw("Not") {
            self.nest()?;
            let e = self.not()?;
            self.depth -= 1;
            return Ok(Expr::Unary(UnOp::Not, Box::new(e)));
        }
        self.compare()
    }
    fn compare(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(
            &[
                ("=", BinOp::Eq),
                ("<>", BinOp::Ne),
                ("<=", BinOp::Le),
                (">=", BinOp::Ge),
                ("<", BinOp::Lt),
                (">", BinOp::Gt),
                ("Like", BinOp::Like),
                ("Is", BinOp::Is),
            ],
            Self::concat,
        )
    }
    fn concat(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("&", BinOp::Concat)], Self::additive)
    }
    fn additive(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("+", BinOp::Add), ("-", BinOp::Sub)], Self::modulo)
    }
    fn modulo(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("Mod", BinOp::Mod)], Self::int_div)
    }
    fn int_div(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("\\", BinOp::IntDiv)], Self::multiplicative)
    }
    fn multiplicative(&mut self) -> Result<Expr, VbaError> {
        self.binary_level(&[("*", BinOp::Mul), ("/", BinOp::Div)], Self::unary)
    }
    fn unary(&mut self) -> Result<Expr, VbaError> {
        if self.eat_op("-") {
            self.nest()?;
            let e = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Unary(UnOp::Neg, Box::new(e)));
        }
        if self.eat_op("+") {
            return self.unary();
        }
        self.power()
    }
    fn power(&mut self) -> Result<Expr, VbaError> {
        let mut l = self.primary()?;
        while self.eat_op("^") {
            // `2^-1`: the exponent may be negated.
            let r = if self.eat_op("-") { Expr::Unary(UnOp::Neg, Box::new(self.primary()?)) } else { self.primary()? };
            l = Expr::Binary(BinOp::Pow, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn primary(&mut self) -> Result<Expr, VbaError> {
        let base = match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.pos += 1;
                Expr::Int(n)
            }
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Expr::Num(n)
            }
            Some(Tok::Str(s)) => {
                self.pos += 1;
                Expr::Str(s.into())
            }
            Some(Tok::Date(d)) => {
                self.pos += 1;
                Expr::Date(d)
            }
            Some(Tok::Op("(")) => {
                self.pos += 1;
                let e = self.expr()?;
                self.expect_op(")")?;
                e
            }
            Some(Tok::Op(".")) => {
                self.pos += 1;
                Expr::WithMember(self.ident()?)
            }
            Some(Tok::Bracketed(n)) => {
                self.pos += 1;
                Expr::Name(n)
            }
            Some(Tok::Ident(w)) => {
                self.pos += 1;
                match w.to_ascii_lowercase().as_str() {
                    "true" => Expr::Bool(true),
                    "false" => Expr::Bool(false),
                    "nothing" => Expr::Nothing,
                    "empty" => Expr::Empty,
                    "null" => Expr::Null,
                    "new" => return Ok(Expr::New(self.ident()?)),
                    "addressof" => return Ok(Expr::AddressOf(self.ident()?)),
                    "typeof" => {
                        let e = self.concat()?;
                        self.expect_kw("Is")?;
                        let t = self.ident()?;
                        let t = if self.eat_op(".") { self.ident()? } else { t };
                        return Ok(Expr::TypeOf(Box::new(e), t));
                    }
                    _ if is_reserved(&w) => {
                        self.pos -= 1;
                        return Err(self.err(format!("unexpected `{w}`")));
                    }
                    _ => Expr::Name(w),
                }
            }
            _ => return Err(self.err("expected a value")),
        };
        self.postfix(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stmts(s: &str) -> Vec<Stmt> {
        parse_statements(s).unwrap_or_else(|e| panic!("{e}: {s}"))
    }

    #[test]
    fn calls_assignments_and_named_arguments() {
        let s =
            stmts("Selection.PasteSpecial Paste:=xlPasteValues, Transpose:=False\nRange(\"A1\").Value = 5\nMsgBox \"hi\", vbOKOnly\nFoo\nx.Select");
        assert!(
            matches!(&s[0], Stmt::Call(Expr::Member(_, m), args) if m == "PasteSpecial" && args.len() == 2 && args[0].name.as_deref() == Some("Paste"))
        );
        assert!(matches!(&s[1], Stmt::Let(Expr::Member(_, m), Expr::Int(5)) if m == "Value"));
        assert!(matches!(&s[2], Stmt::Call(Expr::Name(n), args) if n == "MsgBox" && args.len() == 2));
        assert!(matches!(&s[3], Stmt::Call(Expr::Name(n), args) if n == "Foo" && args.is_empty()));
        assert!(matches!(&s[4], Stmt::Call(Expr::Member(_, m), _) if m == "Select"));
    }

    #[test]
    fn control_flow() {
        let s = stmts(
            "For i = 1 To 10 Step 2\n x = x + i\nNext i\nFor Each c In r\nNext\nDo While a < 3\n a = a + 1\nLoop\nIf a Then b = 1 Else b = 2\nIf a = 1 Then\n b\nElseIf a = 2 Then\n c\nElse\n d\nEnd If\nSelect Case x\nCase 1, 2: y = 1\nCase 3 To 5\nCase Is > 10\nCase Else\nEnd Select\nWith Range(\"A1\")\n .Value = 1\n .Font.Bold = True\nEnd With\nOn Error Resume Next\nOn Error GoTo Fail\nExit Sub\nFail:\nResume Next",
        );
        assert!(matches!(&s[0], Stmt::For { step: Some(_), body, .. } if body.len() == 1));
        assert!(matches!(&s[1], Stmt::ForEach { .. }));
        assert!(matches!(&s[2], Stmt::Do { cond: Some((_, false, true)), .. }));
        assert!(matches!(&s[3], Stmt::If { arms, otherwise } if arms.len() == 1 && otherwise.len() == 1));
        assert!(matches!(&s[4], Stmt::If { arms, otherwise } if arms.len() == 2 && otherwise.len() == 1));
        assert!(matches!(&s[5], Stmt::Select { cases, otherwise: Some(_), .. } if cases.len() == 3));
        assert!(matches!(&s[6], Stmt::With(_, body) if body.len() == 2));
        assert!(matches!(&s[7], Stmt::OnError(OnError::ResumeNext)));
        assert!(matches!(&s[8], Stmt::OnError(OnError::GoTo(l)) if l == "Fail"));
        assert!(matches!(&s[10], Stmt::Label(l) if l == "Fail"));
    }

    #[test]
    fn precedence() {
        let s = stmts("x = -2 ^ 2 + 3 * 4 & \"a\" = \"b\" And Not c");
        let Stmt::Let(_, e) = &s[0] else { panic!() };
        assert!(matches!(e, Expr::Binary(BinOp::And, _, _)));
    }

    #[test]
    fn modules_with_declarations() {
        let m = parse_module(
            "Attribute VB_Name = \"Module1\"\r\nOption Explicit\r\n#If VBA7 Then\r\nPrivate Declare PtrSafe Function GetTickCount Lib \"kernel32\" () As Long\r\n#Else\r\nPrivate Declare Function GetTickCount Lib \"kernel32\" () As Long\r\n#End If\r\nPublic total As Long\r\nConst Rate = 0.5\r\nSub Main()\r\n  Dim p1, p2 As Long, s$\r\n  total = 1\r\nEnd Sub\r\nPrivate Function Twice(ByVal n As Long, Optional k = 2) As Long\r\n  Twice = n * k\r\nEnd Function\r\nType Pt\r\n x As Long\r\nEnd Type\r\n",
        )
        .unwrap();
        assert_eq!(m.declares.len(), 1, "one #If branch kept");
        assert_eq!(m.vars.len(), 1);
        assert_eq!(m.consts.len(), 1);
        assert_eq!(m.procs.len(), 2);
        assert_eq!(m.procs[1].params.len(), 2);
        assert!(m.procs[1].params[1].optional);
        assert_eq!(m.types.len(), 1);
    }

    #[test]
    fn broken_code_is_a_syntax_error_with_a_line() {
        match parse_module("Sub A()\r\n  x = (1 +\r\nEnd Sub\r\n") {
            Err(VbaError::Syntax { line, .. }) => assert_eq!(line, 2),
            other => panic!("{other:?}"),
        }
        assert!(parse_module("Sub A()\r\n").is_err());
        // Deep nesting is refused, not a stack overflow.
        let deep = format!("x = {}1{}", "(".repeat(5000), ")".repeat(5000));
        assert!(parse_statements(&deep).is_err());
    }
}

#[cfg(test)]
mod corpus {
    /// Parses every `.bas` file in `$GRIDCRAFT_VBA_CORPUS` (real macros kept outside the repo).
    #[test]
    #[ignore = "needs GRIDCRAFT_VBA_CORPUS"]
    fn corpus_parses() {
        let Some(dir) = std::env::var_os("GRIDCRAFT_VBA_CORPUS") else { return };
        let mut failures = Vec::new();
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let src = std::fs::read_to_string(e.path()).unwrap();
            n += 1;
            if let Err(err) = super::parse_module(&src) {
                failures.push(format!("{}: {err}", e.path().display()));
            }
        }
        assert!(failures.is_empty(), "{} of {n} modules failed:\n{}", failures.len(), failures.join("\n"));
    }
}
