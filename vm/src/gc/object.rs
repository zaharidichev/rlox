use super::arena::ArenaPtr;
use super::value::Value;

use ::chunk::Chunk;

use std::fmt::{Debug, Display};
use std::ops::Deref;
use std::ops::DerefMut;

pub enum Object {
    String(String),
    LoxFunction(LoxFunction),
}

pub struct LoxFunction {
    name: String,
    chunk: Chunk,
    arity: u8,
}

impl LoxFunction {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn chunk(&mut self) -> &mut Chunk {
        &mut self.chunk
    }
}

impl Object {
    pub fn string(string: String) -> Self {
        Object::String(string)
    }

    pub fn function(name: &str, arity: u8, chunk: Chunk) -> Self {
        let name = name.into();
        Object::LoxFunction(
            LoxFunction { name, arity, chunk }
        )
    }

    pub fn is_function(&self) -> bool {
        if let Object::LoxFunction(_) = *self {
            true
        } else {
            false
        }
    }
}

impl Debug for Object {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match *self {
            Object::String(ref s) => write!(f, "<string: {:?}>", s),
            Object::LoxFunction(ref fun) => write!(f, "<function: {:?}>", fun.name),
        }
    }
}

impl Display for Object {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match *self {
            Object::String(ref s) => write!(f, "{}", s),
            Object::LoxFunction(ref fun) => write!(f, "<function {:?}>", fun.name),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct ObjectHandle {
    ptr: ArenaPtr<Object>
}

impl ObjectHandle {
    pub fn new(ptr: ArenaPtr<Object>) -> Self {
        ObjectHandle {ptr}
    }

    pub fn into_value(self) -> Value {
        Value::object(self)
    }

    pub fn into_raw(self) -> u64 {
        self.ptr.into_raw()
    }

    pub fn mark(&mut self) {
        self.ptr.mark();
    }

    pub fn is_marked(&self) -> bool {
        self.ptr.is_marked()
    }
}

impl Deref for ObjectHandle {
    type Target = Object;

    fn deref(&self) -> &Self::Target {
        self.ptr.deref()
    }
}

impl DerefMut for ObjectHandle {
    fn deref_mut(&mut self) -> &mut Object {
        self.ptr.deref_mut()
    }
}

impl Display for ObjectHandle {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "{}", self.deref())
    }
}

impl Debug for ObjectHandle {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "{:?}", self.deref())
    }
}