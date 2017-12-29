use std::fmt::Debug;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;

use value::Value;
use value::LoxInstance;
use environment::Environment;
use parser::ast::FunctionDecl;
use parser::ast::FunctionStmt;

use eval::RuntimeError;
use eval::Interpreter;


#[derive(Clone)]
pub struct LoxClassHandle {
    class: Rc<LoxClass>,
}

use std::ops::Deref;

impl Deref for LoxClassHandle {
    type Target = LoxClass;

    fn deref(&self) -> &LoxClass{
        &*self.class
    }
}

#[derive(Clone)]
pub struct LoxClass {
    name: String,
    methods: HashMap<String, LoxFunction>,
    superclass: Option<LoxClassHandle>,
}

impl LoxClassHandle {
    pub fn call(&self, interpreter: &mut Interpreter, arguments: Vec<Value>) -> Result<Value, RuntimeError> {
        let instance = Value::Instance(LoxInstance::new(self.class.clone()));
        if let Some(init) = self.init() {
            let bound = init.bind(instance.clone());
            bound.call(interpreter, arguments)?;
        }
        Ok(instance)
    }

    fn arity(&self) -> usize {
        self.init().map(|m| m.arity()).unwrap_or(0)
    }
}

impl LoxClass {
    pub fn new(name: &str, method_stmts: Vec<FunctionStmt>, env: Environment, superclass: Option<LoxClassHandle>) -> Self {
        let mut methods = HashMap::new();
        for stmt in method_stmts {
            let mname = stmt.var.name();
            let fun_decl = stmt.declaration.clone();
            let method = LoxFunction::new(mname, fun_decl, env.clone());
            methods.insert(mname.to_owned(), method);
        }
        LoxClass {
            name: name.to_owned(),
            methods: methods,
            superclass: superclass,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn init(&self) -> Option<LoxFunction> {
        self.methods.get("init").map(Clone::clone)
    }

    pub fn method(&self, name: &str) -> Option<LoxFunction> {
        self.methods
            .get(name)
            .map(Clone::clone)
            .or_else(|| {
                // FIXME: Do I need to clone?
                let superclass = self.superclass.clone();
                superclass.and_then(|sc| sc.method(name))
            })
    }
}

impl PartialEq for LoxClassHandle {
    fn eq(&self, other: &LoxClassHandle) -> bool {
        &*self.class as *const _ == &*other.class as *const _
    }
}

impl ::std::fmt::Debug for LoxClassHandle {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "<class '{}'>", self.name())
    }
}
