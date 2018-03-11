use std::collections::HashMap;

use chunk::Chunk;
use gc::Gc;
use gc::object::Object;
use gc::object::ObjectHandle;
use gc::value::Value;
use gc::value::Variant;

pub struct VM {
    // FIXME: Local variables are not currently rooted properly, we will need
    // to scan the stack to address this at this point. If we do this...we can just
    // ignore precise rooting of any form.
    gc: Gc,
    globals: HashMap<String, Value>,

    // TODO: Write a stack facade that hides the vector and call frames,
    // that way we can refer to the slots into the stack safely.

    stack: Vec<Value>,
    frames: Vec<CallFrame>,
}

pub struct CallFrame {
    function: ObjectHandle,
    ip: usize,
    stack_start: usize,
}

impl CallFrame {
    pub fn new(function: ObjectHandle, stack_start: usize) -> Self {
        if !function.is_function() {
            panic!("Callframe must be constructed from a function");
        }
        CallFrame {
            function,
            ip: 0,
            stack_start,
        }
    }

    pub fn read_byte(&mut self) -> u8 {
        self.ip += 1;
        let ip = self.ip;
        self.chunk().get(ip - 1)
    }

    pub fn read_u16(&mut self) -> u16 {
        self.ip += 2;
        let ip = self.ip;
        let chunk = self.chunk();
        let lo = chunk.get(ip - 2) as u16;
        let hi = chunk.get(ip - 1) as u16;
        lo + (hi << 8)
    }

    pub fn read_u64(&mut self) -> u64 {
        self.ip += 8;
        let ip = self.ip;
        let chunk = self.chunk();

        let b1 = chunk.get(ip - 8) as u64;
        let b2 = chunk.get(ip - 7) as u64;
        let b3 = chunk.get(ip - 6) as u64;
        let b4 = chunk.get(ip - 5) as u64;
        let b5 = chunk.get(ip - 4) as u64;
        let b6 = chunk.get(ip - 3) as u64;
        let b7 = chunk.get(ip - 2) as u64;
        let b8 = chunk.get(ip - 1) as u64;
        b1 +
            (b2 << 8) +
            (b3 << 16) +
            (b4 << 24) +
            (b5 << 32) +
            (b6 << 40) +
            (b7 << 48) +
            (b8 << 56)
    }

    pub fn read_constant(&mut self) -> Value {
        let idx = self.read_byte();
        *self.chunk().get_constant(idx).unwrap()
    }

    pub fn chunk(&mut self) -> &mut Chunk {
        if let Object::LoxFunction(ref mut f) = *self.function {
            f.chunk()
        } else {
            unreachable!();
        }
    }
}

impl VM {
    pub fn new(chunk: Chunk, mut gc: Gc) -> Self {
        let obj = Object::function("main", 0, chunk);
        let roots_fn = || {
            // FIXME: This shouldn't be in new anyway because the allocations should happen in the
            // compiler.
            [].into_iter().cloned()
        };

        let function = gc.allocate(obj, roots_fn);

        let frame = CallFrame::new(function, 0);
        let frames = vec![frame];
        VM {
            stack: Vec::new(),
            gc,
            globals: HashMap::new(),
            frames,
        }
    }

    pub fn run(mut self) {
        let l = self.frame_mut().chunk().len();
        while self.frame().ip < l {
            let inst = self.read_byte();
            decode_op!(inst, self);
        }
    }

    fn constant(&mut self, idx: u8) {
        let val = *self.frame_mut().chunk().get_constant(idx).unwrap();
        self.push(val);
    }

    fn print(&mut self) {
        let val = self.pop();
        println!("{}", val);
    }

    fn add(&mut self) {
        let b = self.pop();
        let a = self.pop();
        let c = a.as_float() + b.as_float();
        self.push(Value::float(c));
    }

    fn sub(&mut self) {
        let b = self.pop();
        let a = self.pop();
        let c = a.as_float() - b.as_float();
        self.push(Value::float(c));
    }

    fn mul(&mut self) {
        let b = self.pop();
        let a = self.pop();
        let c = a.as_float() * b.as_float();
        self.push(Value::float(c));
    }

    fn div(&mut self) {
        let b = self.pop();
        let a = self.pop();
        let c = a.as_float() / b.as_float();
        self.push(Value::float(c));
    }

    fn neg(&mut self) {
        let a = self.pop().as_float();
        self.push(Value::float(-a));
    }

    fn not(&mut self) {
        let a = self.pop();
        if a.truthy() {
            self.push(Value::falselit());
        } else {
            self.push(Value::truelit());
        }
    }

    fn eq(&mut self) {
        let b = self.pop();
        let a = self.pop();
        if a == b {
            self.push(Value::truelit());
        } else {
            self.push(Value::falselit());
        }
    }

    fn gt(&mut self) {
        let b = self.pop().as_float();
        let a = self.pop().as_float();
        if a > b {
            self.push(Value::truelit());
        } else {
            self.push(Value::falselit());
        }
    }

    fn lt(&mut self) {
        let b = self.pop().as_float();
        let a = self.pop().as_float();
        if a < b {
            self.push(Value::truelit());
        } else {
            self.push(Value::falselit());
        }
    }

    fn jmp(&mut self) {
        self.frame_mut().ip = self.read_u16() as usize;
    }

    fn jze(&mut self) {
        let ip = self.read_u16();
        if self.peek().falsey() {
            self.frame_mut().ip = ip as usize;
        }
    }

    fn get_global(&mut self) {
        let val = self.frame_mut().read_constant();

        if let Variant::Obj(h) = val.decode() {
            if let Object::String(ref s) = *h {
                let val = *self.globals.get(s).expect("undefined global");
                self.push(val);
                return;
            }
        }
        panic!("GET_GLOBAL constant was not a string");
    }

    fn set_global(&mut self) {
        let val = self.frame_mut().read_constant();

        if let Variant::Obj(h) = val.decode() {
            if let Object::String(ref s) = *h {
                let lhs = self.pop();
                self.globals.insert(s.clone(), lhs);
                self.push(lhs);
                return;
            }
        }
        panic!("SET_GLOBAL constant was not a string");
    }

    fn define_global(&mut self) {
        let val = self.frame_mut().read_constant();

        if let Variant::Obj(h) = val.decode() {
            if let Object::String(ref s) = *h {
                let lhs = self.pop();
                self.globals.insert(s.clone(), lhs);
                return;
            }
        }
        panic!("DEF_GLOBAL constant was not a string");
    }

    fn slots(&self) -> &[Value] {
        let frame = self.frame();
        &self.stack[frame.stack_start..]
    }

    fn slots_mut(&mut self) -> &mut [Value] {
        let start = self.frame().stack_start;
        &mut self.stack[start..]
    }

    fn get_local(&mut self) {
        let idx = self.read_byte();
        // FIXME: When are locals ever even reserved? :(
        let val = self.slots()[idx as usize];
        self.push(val);
    }

    fn set_local(&mut self) {
        let idx = self.read_byte();
        // We peek because we would just push it back after
        // the assignment occurs.
        let val = self.peek();
        self.slots_mut()[idx as usize] = val;
    }

    fn frame(&self) -> &CallFrame {
        self.frames.last().expect("frames to be nonempty")
    }

    fn frame_mut(&mut self) -> &mut CallFrame {
        self.frames.last_mut().expect("frames to be nonempty")
    }

    fn immediate(&mut self) {
        let raw = self.frame_mut().read_u64();
        let val = unsafe { Value::from_raw(raw) };
        self.push(val);
    }

    fn imm_nil(&mut self) {
        self.push(Value::nil());
    }

    fn imm_true(&mut self) {
        self.push(Value::truelit());
    }

    fn imm_false(&mut self) {
        self.push(Value::falselit());
    }

    fn call(&mut self, arity: u8) {
        let last = self.stack.len() - 1;
        let stack_start = last - (arity + 1) as usize;
        let callee = self.stack[stack_start];

        // ensure callee is a function
        if let Variant::Obj(obj) = callee.decode() {
            let frame = CallFrame::new(obj, stack_start);
            self.frames.push(frame);
        } else {
            panic!("Callee was not an object");
        }
    }

    fn ret(&mut self) {
        if let Some(frame) = self.frames.pop() {
            self.stack.truncate(frame.stack_start);
        }
        // Remove all arguments off the stack
        panic!("Cannot return from top-level.");
    }

    fn read_byte(&mut self) -> u8 {
        self.frame_mut().read_byte()
    }

    fn read_u16(&mut self) -> u16 {
        self.frame_mut().read_u16()
    }

    // fn reset_stack(&mut self) {
    // }

    fn push(&mut self, value: Value) {
        self.stack.push(value);
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().unwrap()
    }

    fn peek(&mut self) -> Value {
        self.stack.last()
            .expect("stack to be nonempty")
            .clone()
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        // TODO: Unroot all non-primitive constants.
    }
}
