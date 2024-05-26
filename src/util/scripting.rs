use std::{marker::PhantomData, thread::LocalKey};

use botapi::gen_types::rhai_helpers::setup_all_rhai;
use lazy_static::lazy_static;
use once_cell::sync::Lazy;
use rhai::plugin::*;
use rhai::{export_module, exported_module, Dynamic, Engine, FnPtr, FuncArgs, Scope, AST};
use threadpool::ThreadPool;
use tokio::sync::mpsc;

use crate::persist::admin::actions::ActionType;
use crate::statics::CONFIG;

use super::error::{BotError, Result};

lazy_static! {
    pub static ref COMPUTE_TP: ThreadPool = ThreadPool::new(CONFIG.compute_threads);
}

thread_local! {
    /// Thread local rhai engine preloaded with telegram api types
    pub static RHAI_ENGINE: Lazy<Engine> = Lazy::new(|| {
        let mut engine = Engine::new();
        engine.on_print(|_| ());
        engine.on_debug(|_, _, _| ());
        terminate_on_progress(&mut engine, 1024);
        setup_all_rhai(&mut engine);
        let tg_api = exported_module!(tg_api);
        let action = exported_module!(action);
        engine.register_global_module(tg_api.into());
        engine.register_static_module("ModAction" ,action.into());
        engine.register_type_with_name::<ModAction>("ModAction");
        engine
    });
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ModAction {
    Ignore,
    Delete,
    Reply(String),
    Warn(Option<String>),
    Ban(Option<String>),
    Mute(Option<String>),
}

impl ModAction {
    pub fn is_delete(&self) -> bool {
        matches!(self, Self::Ignore)
    }

    pub fn get_action_type(&self) -> Option<ActionType> {
        match self {
            Self::Reply(_) => None,
            Self::Ignore => None,
            Self::Delete => Some(ActionType::Delete),
            Self::Warn(_) => Some(ActionType::Warn),
            Self::Ban(_) => Some(ActionType::Ban),
            Self::Mute(_) => Some(ActionType::Mute),
        }
    }

    pub fn to_reason(self) -> Option<String> {
        match self {
            Self::Reply(r) => Some(r),
            Self::Ignore => None,
            Self::Delete => None,
            Self::Mute(r) => r,
            Self::Ban(r) => r,
            Self::Warn(r) => r,
        }
    }

    // pub fn with_reason(self) -> Option<(ActionType, Option<String>)> {
    //     match self {
    //         Self::Ignore => None,
    //         Self::Reply()
    //     }
    // }
}

#[allow(non_upper_case_globals, non_snake_case)]
#[export_module]
mod action {
    use rhai::Dynamic;

    use super::ModAction;

    pub const Ignore: ModAction = ModAction::Ignore;

    pub const Delete: ModAction = ModAction::Delete;

    pub fn Reply(value: String) -> ModAction {
        ModAction::Reply(value)
    }

    pub fn Warn(value: Dynamic) -> ModAction {
        if let Some(value) = value.try_cast::<String>() {
            ModAction::Warn(Some(value))
        } else {
            ModAction::Warn(None)
        }
    }

    pub fn Ban(value: Dynamic) -> ModAction {
        if let Some(value) = value.try_cast::<String>() {
            ModAction::Ban(Some(value))
        } else {
            ModAction::Ban(None)
        }
    }

    pub fn Mute(value: Dynamic) -> ModAction {
        if let Some(value) = value.try_cast::<String>() {
            ModAction::Mute(Some(value))
        } else {
            ModAction::Mute(None)
        }
    }

    /// Return the current variant of `MyEnum`.
    #[rhai_fn(global, get = "enum_type", pure)]
    pub fn get_type(my_enum: &mut ModAction) -> String {
        match my_enum {
            ModAction::Ignore => "Ignore".to_string(),
            ModAction::Delete => "Delete".to_string(),
            ModAction::Reply(_) => "Reply".to_string(),
            ModAction::Warn(_) => "Warn".to_string(),
            ModAction::Ban(_) => "Ban".to_string(),
            ModAction::Mute(_) => "Mute".to_owned(),
        }
    }

    /// Return the inner value.
    #[rhai_fn(global, get = "value", pure)]
    pub fn get_value(my_enum: &mut ModAction) -> Dynamic {
        match my_enum {
            ModAction::Ignore => Dynamic::UNIT,
            ModAction::Delete => Dynamic::UNIT,
            ModAction::Reply(x) => Dynamic::from(x.clone()),
            ModAction::Warn(x) => Dynamic::from(x.clone()),
            ModAction::Ban(x) => Dynamic::from(x.clone()),
            ModAction::Mute(x) => Dynamic::from(x.clone()),
        }
    }

    // Printing
    #[rhai_fn(global, name = "to_string", pure)]
    pub fn to_string(my_enum: &mut ModAction) -> String {
        format!("{my_enum:?}")
    }

    #[rhai_fn(global, name = "to_debug", pure)]
    pub fn to_debug(my_enum: &mut ModAction) -> String {
        format!("{:?}", my_enum)
    }
}
#[export_module]
mod tg_api {
    use crate::util::glob::WildMatch;
    use rhai::Dynamic;

    pub fn glob(value: &str, matches: &str) -> bool {
        WildMatch::new(value).matches(matches)
    }
}

pub trait EngineGetter: Send {
    fn run<F, R>(&'_ self, handler: F) -> R
    where
        for<'b> F: FnOnce(&'b Engine) -> R;

    fn run_mut<F, R>(&'_ mut self, handler: F) -> R
    where
        for<'b> F: FnOnce(&'b Engine) -> R;
}

impl<'a> EngineGetter for &'a Engine {
    fn run<F, R>(&'_ self, handler: F) -> R
    where
        for<'b> F: FnOnce(&'b Engine) -> R,
    {
        handler(self)
    }

    fn run_mut<F, R>(&'_ mut self, handler: F) -> R
    where
        for<'b> F: FnOnce(&'b Engine) -> R,
    {
        handler(self)
    }
}

impl EngineGetter for Engine {
    fn run<F, R>(&'_ self, handler: F) -> R
    where
        for<'b> F: FnOnce(&'b Engine) -> R,
    {
        handler(self)
    }

    fn run_mut<F, R>(&'_ mut self, handler: F) -> R
    where
        for<'b> F: FnOnce(&'b Engine) -> R,
    {
        handler(self)
    }
}

impl EngineGetter for &'static LocalKey<Lazy<Engine>>
where
    Self: 'static,
{
    fn run<F, R>(&'_ self, handler: F) -> R
    where
        for<'b> F: FnOnce(&'b Engine) -> R,
    {
        self.with(|v| handler(v))
    }
    fn run_mut<F, R>(&'_ mut self, handler: F) -> R
    where
        for<'b> F: FnOnce(&'b Engine) -> R,
    {
        self.with(|v| handler(v))
    }
}

/// Wrapper around rhai engine managing execution limits, scope
/// and threading
pub struct ManagedRhai<'a, T, F> {
    engine: F,
    script: String,
    execution_cap: Option<u64>,
    scope: Option<Scope<'static>>,
    mapper_args: Option<T>,
    expr: bool,
    phantom: PhantomData<&'a ()>,
}

impl<'a, F> ManagedRhai<'a, (), F>
where
    F: EngineGetter,
{
    /// Create a new script without arguments
    pub fn new(script: String, engine: F) -> Self {
        Self {
            engine,
            script,
            execution_cap: None,
            scope: None,
            mapper_args: None,
            expr: false,
            phantom: PhantomData,
        }
    }
}

fn terminate_on_progress(engine: &mut Engine, cap: u64) {
    engine.on_progress(move |p| if p >= cap { Some(Dynamic::UNIT) } else { None });
}

impl<'a, A, F> ManagedRhai<'a, A, F>
where
    F: EngineGetter,
{
    /// Create a new mapper function, anonymous or other wise. The function
    /// is called with the given args
    pub fn new_mapper(script: String, engine: F, args: A) -> Self
    where
        A: FuncArgs,
    {
        Self {
            engine,
            script,
            execution_cap: None,
            scope: None,
            mapper_args: Some(args),
            expr: false,
            phantom: PhantomData,
        }
    }

    /// Terminate the script after a set number of operations
    pub fn execution_cap(mut self, cap: u64) -> Self {
        self.execution_cap = Some(cap);
        self
    }

    /// Run the script as an rhai expression only
    pub fn expression(mut self, expr: bool) -> Self {
        self.expr = expr;
        self
    }

    /// Run the script with a given scope
    pub fn scope(mut self, scope: Scope<'static>) -> Self {
        self.scope = Some(scope);
        self
    }

    fn run_script<T>(&mut self) -> Result<T>
    where
        T: Send + Sync + Clone + 'static,
    {
        let r = self.engine.run_mut(|engine| {
            if let Some(scope) = self.scope.as_mut() {
                engine.eval_with_scope(scope, &self.script)
            } else {
                engine.eval(&self.script)
            }
        })?;
        Ok(r)
    }

    fn eval_expression<T>(&mut self) -> Result<T>
    where
        T: Send + Sync + Clone + 'static,
    {
        let v = self.engine.run_mut(|engine| {
            if let Some(scope) = self.scope.as_mut() {
                engine.eval_expression_with_scope(scope, &self.script)
            } else {
                engine.eval_expression(&self.script)
            }
        })?;
        Ok(v)
    }

    fn run_mapper_expression<T, R>(&mut self, args: T) -> Result<R>
    where
        T: FuncArgs,
        R: Send + Sync + Clone + 'static,
    {
        let r = self.engine.run_mut(|engine| {
            let r = if let Some(scope) = self.scope.as_mut() {
                let ast = engine.compile_expression_with_scope(scope, &self.script)?;
                let fn_ptr: FnPtr = engine.eval_ast_with_scope(scope, &ast)?;
                fn_ptr.call(engine, &ast, args)?
            } else {
                let ast = engine.compile_expression(&self.script)?;
                let fn_ptr: FnPtr = engine.eval_ast(&ast)?;
                fn_ptr.call(engine, &ast, args)?
            };
            Ok::<R, BotError>(r)
        })?;
        Ok(r)
    }

    fn run_mapper<T, R>(&mut self, args: T) -> Result<R>
    where
        T: FuncArgs,
        R: Send + Sync + Clone + 'static,
    {
        let r = self.engine.run_mut(|engine| {
            let r = if let Some(scope) = self.scope.as_mut() {
                let ast = engine.compile_with_scope(scope, &self.script)?;
                let fn_ptr: FnPtr = engine.eval_ast_with_scope(scope, &ast)?;
                fn_ptr.call(engine, &ast, args)?
            } else {
                let ast = engine.compile(&self.script)?;
                let fn_ptr: FnPtr = engine.eval_ast(&ast)?;
                fn_ptr.call(engine, &ast, args)?
            };
            Ok::<R, BotError>(r)
        })?;
        Ok(r)
    }

    /// Run this script on the current thread in a blocking fashion
    pub fn run<R>(&mut self) -> Result<R>
    where
        R: Send + Sync + Clone + 'static,
        A: FuncArgs + 'a,
    {
        match (self.mapper_args.take(), self.expr) {
            (Some(args), true) => self.run_mapper_expression(args),
            (Some(args), false) => self.run_mapper(args),
            (None, true) => self.eval_expression(),
            (None, false) => self.run_script(),
        }
    }

    pub fn compile(&self) -> Result<AST> {
        let r = self
            .engine
            .run(|engine| match (self.scope.as_ref(), self.expr) {
                (Some(scope), true) => engine.compile_expression_with_scope(scope, &self.script),
                (Some(scope), false) => engine.compile_with_scope(scope, &self.script),
                (None, true) => engine.compile_expression(&self.script),
                (None, false) => engine.compile(&self.script),
            })?;

        Ok(r)
    }
}

impl<A, F> ManagedRhai<'static, A, F>
where
    F: EngineGetter + 'static,
{
    /// Post this script to the compute threadpool and asynchronously await
    /// the result
    pub async fn post<R>(mut self) -> Result<R>
    where
        R: Send + Sync + Clone + 'static,
        A: FuncArgs + Send + Sync + 'static,
    {
        let (tx, mut rx) = mpsc::channel(1);
        COMPUTE_TP.execute(move || {
            let res = self.run();
            if let Err(err) = tx.blocking_send(res) {
                log::warn!("failed to send script result: {}", err);
            }
        });

        rx.recv()
            .await
            .ok_or_else(|| BotError::Generic("empty result".to_owned()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_script() {
        let r: i64 = ManagedRhai::new("1+1".to_owned(), &Engine::new())
            .run()
            .unwrap();

        assert_eq!(r, 2);
    }

    #[test]
    fn global_engine() {
        let r: i64 = ManagedRhai::new("1+1".to_owned(), &RHAI_ENGINE)
            .run()
            .unwrap();

        assert_eq!(r, 2);
    }

    #[test]
    fn execution_cap() {
        let mut engine = Engine::new();
        terminate_on_progress(&mut engine, 1);
        let r: Result<()> =
            ManagedRhai::new("print(6*6+1); print(\"hello\")".to_owned(), &engine).run();

        assert!(r.is_err());
    }

    #[tokio::test]
    async fn post() {
        let r: i64 = ManagedRhai::new("1+1".to_owned(), Engine::new())
            .post()
            .await
            .unwrap();

        assert_eq!(r, 2);
    }
}
