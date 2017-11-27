/// Parsing lox expressions from a series of Tokens.
///
/// # The Lox Grammar
///
/// The currently supported grammar that we use, taken straight from
/// Crafting Interpreters.
///
/// ```
/// expression → equality
/// assignment → identifier "=" assignment
///            | logic_or ;
/// logic_or   → logic_and ( "or" logic_and )* ;
/// logic_and  → equality ( "and" equality )* ;
/// equality   → comparison ( ( "!=" | "==" ) comparison )*
/// comparison → term ( ( ">" | ">=" | "<" | "<=" ) term )*
/// term       → factor ( ( "-" | "+" ) factor )*
/// factor     → unary ( ( "/" | "*" ) unary )*
/// unary      → ( "!" | "-" ) unary
///            | primary
/// primary    → NUMBER | STRING | "false" | "true" | "nil"
///            | "(" expression ")"
/// ```

use std::iter::Peekable;

use self::errors::*;
use value::Value;
use self::scanner::Scanner;
use self::ast::{Expr, Stmt};

use self::scanner::Token;
use self::scanner::TokenType;
pub use self::scanner::Keyword;

pub mod ast;
pub mod errors;
mod scanner;

pub struct Parser<'t> {
    scanner: Peekable<Scanner<'t>>,
}

// Encapsulates rules with the following form:
// name   → inner ( ( opA | obB | ... ) inner )*
macro_rules! __binary_rule (
    ($name:ident, $inner:ident, $convert:ident, $cons:expr, $($pattern:pat)|*) => (
        fn $name(&mut self) -> Result<Expr<'t>> {
            let mut expr = self.$inner()?;
            while let Ok(ty) = self.peek_type() {
                let tok = match ty {
                    $($pattern)|* => { self.advance()? },
                    _ => break,
                };
                let rhs = self.$inner()?;
                let operator = tok.ty.$convert().unwrap();
                expr = $cons(operator, expr, rhs);
            }
            Ok(expr)
        }
    );
);

macro_rules! logical_impl (
    ($name:ident, $inner:ident, $($pattern:pat)|*) => (
        __binary_rule!($name, $inner, into_logical, Expr::logical, $($pattern)|*);
    );
);

macro_rules! binary_impl (
    ($name:ident, $inner:ident, $($pattern:pat)|*) => (
        __binary_rule!($name, $inner, into_binary, Expr::binary, $($pattern)|*);
    );
);

impl<'t> Iterator for Parser<'t> {
    type Item = Result<Stmt<'t>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.has_next() {
            Some(self.parse_statement())
        } else {
            None
        }
    }
}

impl<'t> Parser<'t> {
    pub fn new(program: &'t str) -> Self {
        let scanner = Scanner::new(program);
        Parser {
            scanner: scanner.peekable(),
        }
    }

    pub fn parse(&mut self) -> ::std::result::Result<Vec<Stmt<'t>>, Vec<SyntaxError>> {
        let mut statements = Vec::new();
        let mut errors = Vec::new();
        for res in self {
            match res {
                Ok(stmt) => statements.push(stmt),
                Err(err) => errors.push(err),
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(statements)
    }

    // program → declaration* eof ;
    pub fn parse_statement(&mut self) -> Result<Stmt<'t>> {
        match self.declaration() {
            Ok(stmt) => Ok(stmt),
            Err(err) => {
                self.synchronize();
                Err(err)
            }
        }
    }

    // declaration → varDecl
    //             | statement ;
    fn declaration(&mut self) -> Result<Stmt<'t>> {
        if let TokenType::Keyword(Keyword::Var) = self.peek_type()? {
            self.advance()?;
            return self.var_decl();
        }
        self.statement()
    }

    // varDecl → "var" IDENTIFIER ( "=" expression )? ";" ;
    fn var_decl(&mut self) -> Result<Stmt<'t>> {
        let ident = self.expect(TokenType::Identifier, "keyword 'var'")?;
        let mut initializer = Expr::Literal(Value::Nil);
        if let TokenType::Equal = self.peek_type()? {
            self.advance()?;
            initializer = self.expression()?;
        }
        self.expect(TokenType::Semicolon, "variable declaration")?;
        Ok(Stmt::Var(ident.value, initializer))
    }

    // statement  → exprStmt
    //            | ifStmt
    //            | printStmt
    //            | block ;
    fn statement(&mut self) -> Result<Stmt<'t>> {
        match self.peek_type()? {
            TokenType::Keyword(Keyword::While) => {
                self.advance()?;
                self.while_statement()
            },
            TokenType::Keyword(Keyword::Print) => {
                self.advance()?;
                self.print_statement()
            },
            TokenType::Keyword(Keyword::If) => {
                self.advance()?;
                self.if_statement()
            },
            TokenType::Keyword(Keyword::For) => {
                self.advance()?;
                self.for_statement()
            },
            TokenType::LeftBrace => {
                self.advance()?;
                self.block()
            },
            _ => self.expression_statement()
        }
    }

    fn print_statement(&mut self) -> Result<Stmt<'t>> {
        let value = self.expression()?;
        self.expect(TokenType::Semicolon, "value")?;
        Ok(Stmt::Print(value))
    }

    fn if_statement(&mut self) -> Result<Stmt<'t>> {
        self.expect(TokenType::LeftParen, "if")?;
        let cond = self.expression()?;
        self.expect(TokenType::RightParen, "if condition")?;
        let then_clause = self.declaration()?;
        if let TokenType::Keyword(Keyword::Else) = self.peek_type()? {
            self.advance()?;
            let else_clause = self.declaration()?;
            Ok(Stmt::if_else_stmt(cond, then_clause, else_clause))
        } else {
            Ok(Stmt::if_stmt(cond, then_clause))
        }
    }

    fn while_statement(&mut self) -> Result<Stmt<'t>> {
        self.expect(TokenType::LeftParen, "while")?;
        let cond = self.expression()?;
        self.expect(TokenType::RightParen, "while condition")?;
        let body = self.statement()?;
		Ok(Stmt::While(cond, Box::new(body)))
    }

    fn for_statement(&mut self) -> Result<Stmt<'t>> {
        self.expect(TokenType::LeftParen, "for")?;
        let init = match self.peek_type()? {
            TokenType::Semicolon => {
                self.advance()?;
                None
            },
            TokenType::Keyword(Keyword::Var) => {
                self.advance()?;
                Some(self.var_decl()?)
            },
            _ => Some(self.expression_statement()?),
        };

        let condition = match self.peek_type()? {
            TokenType::Semicolon => Expr::Literal(Value::True),
            _ => self.expression()?,
        };
        self.expect(TokenType::Semicolon, "for condition")?;

        let increment = match self.peek_type()? {
            TokenType::RightParen => None,
            _ => Some(self.expression()?),
        };
        self.expect(TokenType::RightParen, "for clause")?;

        let mut body = self.statement()?;

        // Desugar into while loop
        body = match increment {
            Some(increment) => {
                Stmt::Block(vec![body, Stmt::Expr(increment)])
            },
            None => body,
        };

        let while_loop = Stmt::While(condition, Box::new(body));
        match init {
            Some(init) => Ok(Stmt::Block(vec![init, while_loop])),
            None => Ok(while_loop),
        }
    }

    // block  → '{' declaration * '}'
    fn block(&mut self) -> Result<Stmt<'t>> {
        let mut block = Vec::new();
        loop {
            if let TokenType::RightBrace = self.peek_type()? {
                self.advance()?;
                return Ok(Stmt::Block(block));
            }
            let stmt = self.declaration()?;
            block.push(stmt);
        }
    }

    fn expect(&mut self, token_type: TokenType, before: &'static str) -> Result<Token<'t>> {
        if self.peek_type()? == token_type {
            let tok = self.advance()?;
            Ok(tok)
        } else {
            Err(SyntaxError::Missing(token_type.to_string(), before))
        }
    }

    fn expression_statement(&mut self) -> Result<Stmt<'t>> {
        let expr = self.expression()?;
        self.expect(TokenType::Semicolon, "value")?;
        Ok(Stmt::Expr(expr))
    }

    // expression → equality
    pub fn expression(&mut self) -> Result<Expr<'t>> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr<'t>> {
        let expr = self.logical_or()?;
        if let TokenType::Equal = self.peek_type()? {
            self.advance()?;
            let value = self.assignment()?;
            if let Expr::Var(name) = expr {
                return Ok(Expr::Assign(name, Box::new(value)));
            }
            return Err(SyntaxError::InvalidAssignment);
        }
        Ok(expr)
    }

    logical_impl!(logical_or, logical_and, TokenType::Keyword(Keyword::Or));
    logical_impl!(logical_and, equality, TokenType::Keyword(Keyword::And));

    // equality   → comparison ( ( "!=" | "==" ) comparison )*
    binary_impl!(equality, comparison, TokenType::BangEq | TokenType::EqualEq);
    // comparison → term ( ( ">" | ">=" | "<" | "<=" ) term )*
    binary_impl!(comparison, term,
        TokenType::GreaterThan
        | TokenType::GreaterThanEq
        | TokenType::LessThan
        | TokenType::LessThanEq);
    // term       → factor ( ( "-" | "+" ) factor )*
    binary_impl!(term, factor, TokenType::Plus | TokenType::Minus);
    // factor     → unary ( ( "/" | "*" ) unary )*
    binary_impl!(factor, unary, TokenType::Slash | TokenType::Star);

    // unary      → ( "!" | "-" ) unary
    //            | primary
    fn unary(&mut self) -> Result<Expr<'t>> {
        match self.peek_type()? {
            TokenType::Bang | TokenType::Minus => {
                let tok = self.advance()?;
                let operator = tok.ty.into_unary().unwrap();
                let unary = self.unary()?;

                Ok(Expr::unary(operator, unary))
            }
            _ => self.primary()
        }
    }

    // primary    → NUMBER | STRING | "false" | "true" | "nil"
    //            | "(" expression ")"
    //            | IDENTIFIER
    fn primary(&mut self) -> Result<Expr<'t>> {
        let peek_type = self.peek_type()?;
        match peek_type {
            TokenType::Keyword(Keyword::Nil) => {
                self.advance()?;
                Ok(Expr::Literal(Value::Nil))
            },
            TokenType::Keyword(Keyword::True) => {
                self.advance()?;
                Ok(Expr::Literal(Value::True))
            },
            TokenType::Keyword(Keyword::False) => {
                self.advance()?;
                Ok(Expr::Literal(Value::False))
            },
            TokenType::String(s) => {
                self.advance()?;
                Ok(Expr::Literal(Value::String(s.into())))
            },
            TokenType::Number(n) => {
                self.advance()?;
                Ok(Expr::Literal(Value::Number(n)))
            },
            TokenType::LeftParen => {
                self.advance()?;
                let expr = self.expression()?;
                self.expect(TokenType::RightParen, "expression")?;
                Ok(Expr::Grouping(Box::new(expr)))
            },
            TokenType::Identifier => {
                let token = self.advance()?;
                Ok(Expr::Var(token.value))
            },
            _ => Err(SyntaxError::PrimaryFailure)
        }
    }

    /// Discards tokens until a statement or expression boundary.
    fn synchronize(&mut self) {
        while let Ok(ty) = self.peek_type() {
            match ty {
                TokenType::Semicolon => {
                    self.advance().unwrap();
                    return;
                },
                TokenType::Keyword(Keyword::Class)
                | TokenType::Keyword(Keyword::Fun)
                | TokenType::Keyword(Keyword::Var)
                | TokenType::Keyword(Keyword::For)
                | TokenType::Keyword(Keyword::If)
                | TokenType::Keyword(Keyword::While)
                | TokenType::Keyword(Keyword::Return)
                | TokenType::Keyword(Keyword::Print)
                | TokenType::EOF => return,
                _ => { self.advance().unwrap(); },
            }
        }
    }

    fn advance(&mut self) -> Result<Token<'t>> {
        // FIXME: Don't unwrap
        self.scanner.next().unwrap_or_else(|| Err(SyntaxError::UnexpectedEOF))
    }

    fn has_next(&mut self) -> bool {
        match self.peek_type() {
            Ok(ty) => ty != TokenType::EOF,
            Err(_) => true,
        }
    }

    fn peek_type(&mut self) -> Result<TokenType<'t>> {
        match self.scanner.peek() {
            Some(&Ok(tok)) => Ok(tok.ty),
            Some(&Err(ref err)) => Err(err.clone()),
            None => Ok(TokenType::EOF),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Parser;
    use super::ast::{UnaryOperator,BinaryOperator,Stmt,Expr};
    use super::ast::dsl::*;

    #[test]
    fn expression() {
        let expressions = [
            "1",
            "1 + -1 * (1 + 1)"
        ];
        for expr in &expressions {
            Parser::new(expr).expression().unwrap();
        }
    }

    #[test]
    fn expression_statement() {
        let prog = r#"
        1;
        "foobar";
        print (1 + 2) * -3;
        "#;

        let mut parser = Parser::new(prog);
        let statements = parser.parse().unwrap();
        assert_eq!(vec![
            Stmt::Expr(number(1.0)),
            Stmt::Expr(string("foobar")),
            Stmt::Print(binary(
                BinaryOperator::Star,
                grouping(binary(
                    BinaryOperator::Plus,
                    number(1.0),
                    number(2.0),
                )),
                unary(UnaryOperator::Minus, number(3.0)),
            )),
        ], statements);
    }

    #[test]
    fn declaration() {
        let prog = r#"
        var a;
        var b = 1 + 1;
        a = 1;
        "#;

        let mut parser = Parser::new(prog);
        let statements = parser.parse().unwrap();
        assert_eq!(vec![
            Stmt::Var("a", nil()),
            Stmt::Var("b", binary(BinaryOperator::Plus, number(1.0), number(1.0))),
            Stmt::Expr(Expr::Assign("a", Box::new(number(1.0))))
        ], statements);
    }

    #[test]
    fn block() {
        let prog = r#"
        {
            var a = 1;
            print a;
        }
        "#;

        let mut parser = Parser::new(prog);
        let statements = parser.parse().unwrap();
        assert_eq!(vec![
            Stmt::Block(vec![
                Stmt::Var("a", number(1.0)),
                Stmt::Print(Expr::Var("a")),
            ])
        ], statements);
    }

    #[test]
    fn synchronize() {
        let prog = r#"
        var a;
        var b = 1 + 1;
        "#;

        let mut parser = Parser::new(prog);
        let statements = parser.parse().unwrap();
        assert_eq!(vec![
            Stmt::Var("a", nil()),
            Stmt::Var("b", binary(BinaryOperator::Plus, number(1.0), number(1.0)))
        ], statements);
    }

    #[test]
    fn parse_with_comments() {
        let prog = r#"
        // Some insightful remark
        1 + 1;
        "#;

        let mut parser = Parser::new(prog);
        let statements = parser.parse().unwrap();
        assert_eq!(vec![
            Stmt::Expr(binary(
                BinaryOperator::Plus,
                number(1.0),
                number(1.0),
            )),
        ], statements);
    }

    #[test]
    fn parse_if_statement() {
        let prog = r#"
        if (true)
            print "this";
        else
            print "that";
        "#;

        let mut parser = Parser::new(prog);
        let statements = parser.parse().unwrap();
        assert_eq!(vec![
            Stmt::if_else_stmt(
                truelit(),
                Stmt::Print(string("this")),
                Stmt::Print(string("that"))
            )
        ], statements);
    }

    #[test]
    fn parse_while_loop() {
        let prog = r#"
        while (a > 1) {
            print "body";
        }
        "#;

        let mut parser = Parser::new(prog);
        let statements = parser.parse().unwrap();
        assert_eq!(vec![
            Stmt::While(
                binary(BinaryOperator::GreaterThan, var("a"), number(1.0)),
                Box::new(Stmt::Block(vec![
                    Stmt::Print(string("body"))
                ]))
            )
        ], statements);
    }
}
