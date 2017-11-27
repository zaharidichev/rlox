use environment::Environment;
use errors::*;
use parser::ast::{Expr,Stmt,Binary,Unary,UnaryOperator,BinaryOperator,Logical,LogicalOperator};
use value::Value;

pub trait Context {
    fn println(&mut self, &Value);
    fn env(&self) -> &Environment;
    fn env_mut(&mut self) -> &mut Environment;
    fn push_env(&mut self);
    fn pop_env(&mut self);
}

pub struct StandardContext {
    environment: Environment,
}

impl StandardContext {
    pub fn new() -> Self {
        StandardContext {
            environment: Environment::new()
        }
    }
}

impl Context for StandardContext {
    fn env(&self) -> &Environment {
        &self.environment
    }

    fn env_mut(&mut self) -> &mut Environment {
        &mut self.environment
    }

    fn push_env(&mut self) {
        self.environment = self.environment.extend();
    }

    fn pop_env(&mut self) {
        if let Some(p) = self.environment.parent() {
            self.environment = p;
        }
    }

    fn println(&mut self, v: &Value) {
        println!("{}", v);
    }
}

pub trait Eval {
    fn eval(&self, context: &mut Context) -> Result<Value>;
}

impl<'t> Eval for Stmt<'t> {
    fn eval(&self, context: &mut Context) -> Result<Value> {
        match *self {
            Stmt::Expr(ref inner) => { inner.eval(context)?; }
            Stmt::Print(ref inner) => {
                let evald = inner.eval(context)?;
                context.println(&evald);
            },
            Stmt::Var(ref var, ref expr) => {
                let val = expr.eval(context)?;
                debug!("Set var '{}' to value {}", var, val);
                context.env_mut().bind(var, val);
            },
            Stmt::Block(ref stmts) => {
                context.push_env();
                for inner in stmts.iter() {
                    inner.eval(context)?;
                }
                context.pop_env();
            },
            Stmt::If(ref cond, ref then_clause, ref else_clause) => {
                if cond.eval(context)?.truthy() {
                    then_clause.eval(context)?;
                } else if let &Some(ref else_clause) = else_clause {
                    else_clause.eval(context)?;
                }
            },
			Stmt::While(ref cond, ref body) => {
				while cond.eval(context)?.truthy() {
					body.eval(context)?;
				}
			}
        }
        Ok(Value::Void)
    }
}

impl<'t> Eval for Expr<'t> {
    fn eval(&self, context: &mut Context) -> Result<Value> {
        match *self {
            Expr::Grouping(ref inner) => inner.eval(context),
            Expr::Logical(ref inner) => inner.eval(context),
            Expr::Binary(ref inner) => inner.eval(context),
            Expr::Unary(ref inner) => inner.eval(context),
            Expr::Literal(ref inner) => inner.eval(context),
            Expr::Var(var) => {
                let env = context.env();
                match env.lookup(var) {
                    None => return Err(ErrorKind::UndefinedVariable(var.into()).into()),
                    Some(v) => {
                        return Ok((*v).clone())
                    }
                }
            },
            Expr::Assign(var, ref lhs) => {
                let lhs = lhs.eval(context)?;
                let env = context.env_mut();
                if env.rebind(var, lhs.clone()) {
                    Ok(lhs)
                } else {
                    Err(ErrorKind::UndefinedVariable(var.into()).into())
                }
            },
        }
    }
}

macro_rules! numeric_binary_op (
    ($op:tt, $lhs:ident, $rhs:ident) => (
        match ($lhs, $rhs) {
            (Value::Number(nlhs), Value::Number(nrhs)) => {
                return Ok(Value::Number(nlhs $op nrhs));
            },
            (Value::Number(_), _) => {
                return Err("Invalid operand on lhs, expected number".into())
            },
            (_, Value::Number(_)) => {
                return Err("Invalid operand on rhs, expected number".into())
            },
            _ => {
                return Err("Invalid operands, expected number or string".into())
            },
        }
    );
);

macro_rules! comparison_op (
    ($op:tt, $lhs:ident, $rhs:ident) => (
        match ($lhs, $rhs) {
            (Value::Number(nlhs), Value::Number(nrhs)) => {
				if nlhs $op nrhs {
					Ok(Value::True)
				} else {
					Ok(Value::False)
				}
            },
            _ => Err("Invalid operands, expected number".into()),
        }
    );
);

impl<'t> Eval for Binary<'t> {
    fn eval(&self, context: &mut Context) -> Result<Value> {
        let lhs = self.lhs.eval(context)?;
        let rhs = self.rhs.eval(context)?;
        let op = &self.operator;

        match *op {
            BinaryOperator::Plus => match (lhs, rhs) {
                (Value::String(lhs), Value::String(rhs)) => {
                    let mut res = lhs.clone();
                    res.push_str(&rhs);
                    return Ok(Value::String(res));
                },
                (lhs, rhs) => numeric_binary_op!(+, lhs, rhs)
            },
            BinaryOperator::Minus => numeric_binary_op!(-, lhs, rhs),
            BinaryOperator::Star => numeric_binary_op!(*, lhs, rhs),
            BinaryOperator::Slash => numeric_binary_op!(/, lhs, rhs),
            BinaryOperator::GreaterThan => comparison_op!(>, lhs, rhs),
            BinaryOperator::GreaterThanEq => comparison_op!(>=, lhs, rhs),
            BinaryOperator::LessThan => comparison_op!(<, lhs, rhs),
            BinaryOperator::LessThanEq => comparison_op!(<=, lhs, rhs),
            _ => unimplemented!("==, !="),
        }
    }
}

impl<'t> Eval for Logical<'t> {
    fn eval(&self, context: &mut Context) -> Result<Value> {
        let lhs = self.lhs.eval(context)?;
        let lhsb = lhs.truthy();

        let shortcircuit = match self.operator {
            LogicalOperator::And => !lhsb,
            LogicalOperator::Or => lhsb,
        };
        if shortcircuit {
            return Ok(lhs);
        }
        self.rhs.eval(context)
    }
}

impl<'t> Eval for Unary<'t> {
    fn eval(&self, context: &mut Context) -> Result<Value> {
        let operand = self.unary.eval(context)?;
        match self.operator {
            UnaryOperator::Minus => {
                match operand {
                    Value::Number(n) => {
                        Ok(Value::Number(-1.0 * n))
                    },
                    _ => {
                        Err("Invalid operand for operator '-', expected number".into())
                    }
                }
            },
            UnaryOperator::Bang => {
                if operand.truthy() {
                    Ok(Value::False)
                } else {
                    Ok(Value::True)
                }
            },
        }
    }
}

impl Eval for Value {
    fn eval(&self, _: &mut Context) -> Result<Value> {
        Ok(self.clone())
    }
}

impl<'a, 't> Eval for &'a [Stmt<'t>]
{
    fn eval(&self, ctx: &mut Context) -> Result<Value> {
        {
            let stmts = self.into_iter();
            for stmt in stmts {
                stmt.eval(ctx)?;
            }
        }
        Ok(Value::Void)
    }
}
