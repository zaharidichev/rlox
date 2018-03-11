use chunk::{Chunk, Op};

use gc::Gc;
use gc::value::Value;

use parser::ast::*;

pub struct Compiler<'g> {
    chunk: Chunk,
    line: usize,
    gc: &'g mut Gc,
    locals: Vec<(String, usize)>,
    scope_depth: usize,
}

impl<'g> Compiler<'g> {
    pub fn new(gc: &'g mut Gc) -> Self {
        Compiler {
            chunk: Chunk::new("<unnamed chunk>".into()),
            line: 1,
            gc,
            locals: Vec::new(),
            scope_depth: 0,
        }
    }

    pub fn compile(mut self, name: &str, stmts: &[Stmt]) -> Chunk {
        // Singleton values are always the first 3 constants in a chunk.
        self.chunk.set_name(name);
        for stmt in stmts {
            self.compile_stmt(stmt);
        }
        self.chunk
    }

    fn compile_stmt(&mut self, stmt: &Stmt) {
        match *stmt {
            Stmt::Print(ref expr) => {
                self.compile_expr(expr);
                self.emit(Op::Print);
            },
            Stmt::Expr(ref expr) => {
                self.compile_expr(expr);
                self.emit(Op::Pop); // expression is unused
            },
            Stmt::Block(ref stmts) => {
                self.scope_depth += 1;
                for s in stmts {
                    self.compile_stmt(s);
                }
                self.scope_depth -= 1;
            },
            Stmt::If(ref cond, ref then_clause, ref else_clause) => {
                self.compile_expr(cond);
                // Jump to the else clause if false
                let else_jmp = self.emit_jze();
                self.emit(Op::Pop); // condition
                self.compile_stmt(&*then_clause);
                let end_jmp = self.emit_jmp();
                // Jump to just past the else clause from the then clause
                self.patch_jmp(else_jmp);
                self.emit(Op::Pop); // condition
                if let &Some(ref else_clause) = else_clause {
                    self.compile_stmt(&*else_clause);
                }
                self.patch_jmp(end_jmp);
            },
            Stmt::While(ref cond, ref body) => {
                let ip = self.ip(); // remember loop start
                self.compile_expr(cond);
                let end_jmp = self.emit_jze();
                self.emit(Op::Pop); // condition
                self.compile_stmt(body);
                self.emit_jmp_to(ip);
                self.patch_jmp(end_jmp);
            },
            Stmt::Function(ref f) => {
                // let name = f.var.name();
                // let decl = f.declaration.borrow();
                // let parameters = (*decl).parameters;
            },
            Stmt::Return(ref expr) => {
                self.compile_expr(expr);
                self.emit(Op::Return);
            }
            // Stmt::Class(_) => {
            // },
            // Stmt::Break => {
            // },
            Stmt::Var(ref var, ref init) => {
                self.compile_expr(init);
                match var.scope() {
                    Scope::Global => {
                        self.emit(Op::SetGlobal);
                        let idx = self.chunk.string_constant(self.gc, var.name());
                        self.emit_byte(idx);
                    },
                    Scope::Local(d) => {
                        let idx = self.resolve_local(var.name(), d);
                        self.emit(Op::SetLocal);
                        self.emit_byte(idx);
                    },
                }
            }
            ref s => unimplemented!("{:?}", s),
        }
    }

    fn compile_expr(&mut self, expr: &Expr) {
        match *expr {
            Expr::Binary(ref binary) => {
                match binary.operator {
                    BinaryOperator::GreaterThanEq | BinaryOperator::LessThanEq => {
                        // Swap to logically equivalent arguments
                        self.compile_expr(&*binary.rhs);
                        self.compile_expr(&*binary.lhs);
                    }
                    _ => {
                        self.compile_expr(&*binary.lhs);
                        self.compile_expr(&*binary.rhs);
                    }
                }

                match binary.operator {
                    BinaryOperator::Plus => self.emit(Op::Add),
                    BinaryOperator::Minus => self.emit(Op::Subtract),
                    BinaryOperator::Star => self.emit(Op::Multiply),
                    BinaryOperator::Slash => self.emit(Op::Divide),
                    BinaryOperator::Equal => self.emit(Op::Equal),
                    BinaryOperator::GreaterThan => self.emit(Op::GreaterThan),
                    BinaryOperator::LessThan => self.emit(Op::LessThan),
                    BinaryOperator::GreaterThanEq => self.emit(Op::LessThan),
                    BinaryOperator::LessThanEq => self.emit(Op::GreaterThan),
                    BinaryOperator::BangEq => {
                        self.emit(Op::Equal);
                        self.emit(Op::Not);
                    }
                }
            }
            Expr::Grouping(ref group) => self.compile_expr(group),
            Expr::Literal(ref lit) => self.emit_constant(lit),
            Expr::Unary(ref unary) => {
                self.compile_expr(&*unary.unary);
                match unary.operator {
                    UnaryOperator::Minus => self.emit(Op::Negate),
                    UnaryOperator::Bang  => self.emit(Op::Not),
                }
            },
            Expr::Logical(ref logical) => {
                match logical.operator {
                    LogicalOperator::And => self.and(&*logical.lhs, &*logical.rhs),
                    LogicalOperator::Or => self.or(&*logical.lhs, &*logical.rhs),
                }
            },
            Expr::Var(ref var) => {
                match var.scope() {
                    Scope::Global => {
                        self.emit(Op::GetGlobal);
                        let idx = self.chunk.string_constant(self.gc, var.name());
                        self.emit_byte(idx);
                    },
                    Scope::Local(d) => {
                        let idx = self.resolve_local(var.name(), d);
                        self.emit(Op::GetLocal);
                        self.emit_byte(idx);
                    },
                }
            }
            Expr::Assign(ref var, ref expr) => {
                self.compile_expr(expr);
                match var.scope() {
                    Scope::Global => {
                        self.emit(Op::SetGlobal);
                        // This constant should already exist, search the table.
                        let idx = self.chunk.string_constant(self.gc, var.name());
                        self.emit_byte(idx);
                    },
                    Scope::Local(d) => {
                        let idx = self.resolve_local(var.name(), d);
                        self.emit(Op::SetLocal);
                        self.emit_byte(idx);
                    },
                }
            },
            Expr::Call(ref call) => {
                self.compile_expr(&call.callee);
                let arity = call.arguments.len();
                for arg in call.arguments.iter() {
                    self.compile_expr(arg);
                }
                if arity > 8 {
                    panic!("Too many arguments.");
                }
                let op = Op::Call(arity as u8);
                self.emit(op);
            },
            ref e => unimplemented!("{:?}", e),
        }
    }

    fn and(&mut self, lhs: &Expr, rhs: &Expr) {
        self.compile_expr(lhs);
        let short_circuit_jmp = self.emit_jze();
        self.emit(Op::Pop); // left operand
        self.compile_expr(rhs);
        self.patch_jmp(short_circuit_jmp);
    }

    fn or(&mut self, lhs: &Expr, rhs: &Expr) {
        self.compile_expr(lhs);
        let else_jmp = self.emit_jze(); // if false, jump to rhs
        let end_jmp = self.emit_jmp(); // if true, jump to end
        self.patch_jmp(else_jmp);
        self.emit(Op::Pop);
        self.compile_expr(rhs); // left operand
        self.patch_jmp(end_jmp);
    }

    fn resolve_local(&mut self, var: &str, depth: usize) -> u8 {
        // Find the local variable with this depth
        let depth = self.scope_depth - depth;

        for (i, &(ref v, d)) in self.locals.iter().enumerate() {
            if v == var && d == depth {
                return i as u8;
            }
        }
        if self.locals.len() == ::std::u8::MAX as usize {
            panic!("TOO MANY LOCAL VARIABLES");
        }
        self.locals.push((var.into(), depth));

        (self.locals.len() - 1) as u8
    }

    fn emit_constant(&mut self, lit: &Literal) {
        match *lit {
            Literal::Nil => self.emit(Op::Nil),
            Literal::True => self.emit(Op::True),
            Literal::False => self.emit(Op::False),
            Literal::Number(n) => {
                self.emit(Op::Immediate);
                let val = Value::float(n).into_raw();
                let b1 = (val & 0xff) as u8;
                let b2 = ((val >> 8) & 0xff) as u8;
                let b3 = ((val >> 16) & 0xff) as u8;
                let b4 = ((val >> 24) & 0xff) as u8;
                let b5 = ((val >> 32) & 0xff) as u8;
                let b6 = ((val >> 40) & 0xff) as u8;
                let b7 = ((val >> 48) & 0xff) as u8;
                let b8 = ((val >> 56) & 0xff) as u8;
                self.chunk.write_byte(b1);
                self.chunk.write_byte(b2);
                self.chunk.write_byte(b3);
                self.chunk.write_byte(b4);
                self.chunk.write_byte(b5);
                self.chunk.write_byte(b6);
                self.chunk.write_byte(b7);
                self.chunk.write_byte(b8);
            }
            Literal::String(ref s) => {
                let idx = self.chunk.string_constant(self.gc, s);
                self.emit(Op::Constant(idx));
            }
        }
    }

    // FIXME: The high-level global ops should have this in their repr, or
    // we should make emit_constant handle the case without OP_CONSTANT
    fn emit_byte(&mut self, byte: u8) {
        self.chunk.write_byte(byte);
    }

    fn emit_jze(&mut self) -> usize {
        self.chunk.write(Op::JumpIfFalse, self.line);
        self.chunk.write_byte(0xff);
        self.chunk.write_byte(0xff);
        self.chunk.len() - 2
    }

    fn emit_jmp(&mut self) -> usize {
        self.chunk.write(Op::Jump, self.line);
        self.chunk.write_byte(0xff);
        self.chunk.write_byte(0xff);
        self.chunk.len() - 2
    }

    fn emit_jmp_to(&mut self, ip: usize) -> usize {
        let lo = (ip & 0xff) as u8;
        let hi = ((ip >> 8) & 0xff) as u8;
        self.chunk.write(Op::Jump, self.line);
        self.chunk.write_byte(lo);
        self.chunk.write_byte(hi);
        self.chunk.len() - 2
    }

    fn ip(&self) -> usize {
        self.chunk.len()
    }

    fn patch_jmp(&mut self, idx: usize) {
        let jmp = self.ip();
        let lo = (jmp & 0xff) as u8;
        let hi = ((jmp >> 8) & 0xff) as u8;
        self.chunk.write_byte_at(idx, lo);
        self.chunk.write_byte_at(idx + 1, hi);
    }

    fn emit(&mut self, op: Op) {
        self.chunk.write(op, self.line);
    }
}
