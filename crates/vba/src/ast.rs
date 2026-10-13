//! VBA syntax tree.

use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Int(i64),
    Num(f64),
    Str(Rc<str>),
    /// A date literal's text (`#1/2/2020#`).
    Date(String),
    Bool(bool),
    Nothing,
    Empty,
    Null,
    /// An omitted argument (`f(1, , 3)`).
    Missing,
    /// A name.
    Name(String),
    /// `.Member` inside a `With` block.
    WithMember(String),
    /// `obj.Member`.
    Member(Box<Expr>, String),
    /// `target(args)`: a call, an array element or a default-member call.
    Call(Box<Expr>, Vec<Arg>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `New ClassName`.
    New(String),
    /// `TypeOf x Is Name`.
    TypeOf(Box<Expr>, String),
    /// `AddressOf Proc` (only meaningful to Windows API calls; evaluates to 0).
    AddressOf(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    /// `name:=value`.
    pub name: Option<String>,
    pub value: Expr,
    /// `ByVal x` in a call (rare).
    pub by_val: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Pow,
    Mul,
    Div,
    IntDiv,
    Mod,
    Add,
    Sub,
    Concat,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Like,
    Is,
    And,
    Or,
    Xor,
    Eqv,
    Imp,
}

/// A declared type (`As Long`); `None`/`Variant` hold anything.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeName {
    Variant,
    Integer,
    Long,
    LongLong,
    Byte,
    Single,
    Double,
    Currency,
    Decimal,
    Date,
    String,
    Boolean,
    Object,
    /// A class, a user type or a host type (`Range`, `Worksheet`).
    Named(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct VarDecl {
    pub name: String,
    pub ty: TypeName,
    /// Array bounds (`Dim a(1 To 5, 3)`): each (lower, upper); `Some(vec![])` for a dynamic
    /// array (`Dim a()`).
    pub bounds: Option<Vec<(Option<Expr>, Expr)>>,
    /// `Dim x As New Class`.
    pub new: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CaseTest {
    Value(Expr),
    Range(Expr, Expr),
    Is(BinOp, Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum OnError {
    ResumeNext,
    GoTo(String),
    GoToZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitKind {
    Sub,
    Function,
    Property,
    For,
    Do,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Dim(Vec<VarDecl>),
    Static(Vec<VarDecl>),
    Const(Vec<(String, Expr)>),
    ReDim {
        preserve: bool,
        decls: Vec<VarDecl>,
    },
    Erase(Vec<Expr>),
    Let(Expr, Expr),
    Set(Expr, Expr),
    /// A call statement: the target and any arguments written without parentheses.
    Call(Expr, Vec<Arg>),
    If {
        arms: Vec<(Expr, Vec<Stmt>)>,
        otherwise: Vec<Stmt>,
    },
    For {
        var: Expr,
        from: Expr,
        to: Expr,
        step: Option<Expr>,
        body: Vec<Stmt>,
    },
    ForEach {
        var: Expr,
        items: Expr,
        body: Vec<Stmt>,
    },
    /// `Do [While|Until c] … Loop [While|Until c]`: (condition, `true` for Until, `true` when
    /// tested before the body).
    Do {
        cond: Option<(Expr, bool, bool)>,
        body: Vec<Stmt>,
    },
    While(Expr, Vec<Stmt>),
    Select {
        subject: Expr,
        cases: Vec<(Vec<CaseTest>, Vec<Stmt>)>,
        otherwise: Option<Vec<Stmt>>,
    },
    With(Expr, Vec<Stmt>),
    Exit(ExitKind),
    GoTo(String),
    Label(String),
    OnError(OnError),
    Resume(Option<String>),
    /// `End`: stops every running macro.
    End,
    /// Statements GridCraft reads but has nothing to do for (`Option Explicit`, `DoEvents`).
    Nop,
    /// The source line the next statement is on (for error messages).
    Line(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcKind {
    Sub,
    Function,
    PropertyGet,
    PropertyLet,
    PropertySet,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub by_val: bool,
    pub optional: bool,
    pub default: Option<Expr>,
    pub param_array: bool,
    pub ty: TypeName,
    pub array: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    pub name: String,
    pub kind: ProcKind,
    pub private: bool,
    pub params: Vec<Param>,
    pub ret: TypeName,
    pub body: Rc<Vec<Stmt>>,
    /// Source line of the declaration.
    pub line: u32,
}

/// A `Declare` of a DLL function: GridCraft has no DLLs to call, so these return 0.
#[derive(Clone, Debug, PartialEq)]
pub struct Declare {
    pub name: String,
    pub lib: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModuleAst {
    pub vars: Vec<(VarDecl, bool)>,
    pub consts: Vec<(String, Expr, bool)>,
    pub procs: Vec<Proc>,
    pub declares: Vec<Declare>,
    /// `Type … End Type`: name and members.
    pub types: Vec<(String, Vec<VarDecl>)>,
    /// `Enum … End Enum` members as constants.
    pub enums: Vec<(String, Vec<(String, Option<Expr>)>)>,
    pub option_base: i64,
    pub option_compare_text: bool,
}
