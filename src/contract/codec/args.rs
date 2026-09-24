//! Argument lists for calling JavaScript functions: tuples pass one argument per
//! element, slices and vectors one argument per item.

use rquickjs::function::Args;
use rquickjs::{Ctx, Result as JsResult};

use super::super::arity::for_each_tuple;
use super::{JsArgs, JsEncode, at_path};

/// Encodes argument `index` and appends it, prefixing errors with `arguments[index]`.
fn push_argument<'js, T: JsEncode + ?Sized>(
    ctx: &Ctx<'js>,
    args: &mut Args<'js>,
    index: usize,
    argument: &T,
) -> JsResult<()> {
    let value = argument
        .encode_js(ctx)
        .map_err(|error| at_path(error, format_args!("arguments[{index}]")))?;
    args.push_arg(value)
}

impl JsArgs for () {
    fn encode_args<'js>(&self, ctx: &Ctx<'js>) -> JsResult<Args<'js>> {
        Ok(Args::new(ctx.clone(), 0))
    }
}

macro_rules! tuple_args {
    ($len:literal => $($index:tt $name:ident),+) => {
        impl<$($name: JsEncode),+> JsArgs for ($($name,)+) {
            fn encode_args<'js>(&self, ctx: &Ctx<'js>) -> JsResult<Args<'js>> {
                let mut args = Args::new(ctx.clone(), $len);
                $(push_argument(ctx, &mut args, $index, &self.$index)?;)+
                Ok(args)
            }
        }
    };
}

for_each_tuple!(tuple_args);

impl<T: JsEncode> JsArgs for [T] {
    fn encode_args<'js>(&self, ctx: &Ctx<'js>) -> JsResult<Args<'js>> {
        let mut args = Args::new(ctx.clone(), self.len());
        for (index, argument) in self.iter().enumerate() {
            push_argument(ctx, &mut args, index, argument)?;
        }
        Ok(args)
    }
}

impl<T: JsEncode> JsArgs for Vec<T> {
    fn encode_args<'js>(&self, ctx: &Ctx<'js>) -> JsResult<Args<'js>> {
        self.as_slice().encode_args(ctx)
    }
}

impl<A: JsArgs + ?Sized> JsArgs for &A {
    fn encode_args<'js>(&self, ctx: &Ctx<'js>) -> JsResult<Args<'js>> {
        (**self).encode_args(ctx)
    }
}
