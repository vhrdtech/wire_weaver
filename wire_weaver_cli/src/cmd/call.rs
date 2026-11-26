//! `ww call <EXPR>` (TOOL-41): call a method, read a property or write one, on any device, from its introspection
//! data alone. The expression is the one `ww explain` reads: `led_on`, `'set_led(true)'`, `'motor.limit = 3'`,
//! `motor.limit`.

use crate::registry::{BoxFuture, CommandDef, Connection, DeviceUse, GlobalFlags, Ran, Run};
use crate::util::display_path;
use crate::value_fmt::fmt_value;
use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, FromArgMatches};
use tracing::level_filters::LevelFilter;
use wire_weaver_client::expr::{self, Call};
use wire_weaver_client::ww_self::{ArgumentOwned, ValueOwned, check_args_count};
use wire_weaver_client::{DynClient, DynResource, DynResourceKind};

#[derive(Args)]
pub(crate) struct CallArgs {
    /// What to do, as a Rust expression: a method call (`'set_led(true)'`, or `led_on` without arguments), a property
    /// read (`motor.limit`) or a property write (`'motor.limit = 3'`); quote it for the shell
    what: String,
}

/// What `ww call` prints for `what` on `device`: a method's return value, `()` for a method that returns nothing
/// or a write, a property's value for a read. For a program or a test that shows the same text, such as a book's
/// transcript. A method's `Err` is an error, with the value it carries.
pub async fn call(device: &DynClient, what: &str) -> Result<String> {
    let bundle = device
        .device_introspect()
        .ok_or_else(|| anyhow!("the device sent no introspection data: `ww call` needs it"))?
        .api_bundle;
    let root = DynResource::root(bundle.clone());
    let cmd = device.cmd();
    let walk = |path: &str| {
        root.walk(path)
            .with_context(|| format!("'{}'", display_path(path)))
    };
    let (resource, args) = match expr::parse_call(what)? {
        Call::Method { path, args } => (walk(&path)?, Some(args)),
        Call::Write { path, value } => {
            let resource = walk(&path)?;
            let DynResourceKind::Property { ty, .. } = resource.kind()? else {
                bail!("'{}' is not a property", display_path(&path));
            };
            let value = expr::value_from_expr(&value, ty, &bundle)?;
            resource.write(cmd, &value).await?;
            return Ok("()".to_owned());
        }
        Call::Path { path } => (walk(&path)?, None),
    };
    let path = display_path(resource.path());
    match resource.kind()? {
        DynResourceKind::Method {
            args: defs,
            return_ty,
            ..
        } => {
            let exprs = args.unwrap_or_default();
            // a #[flatten] argument's struct fields in its place; trailing arguments with a default can be left out
            let fields = ArgumentOwned::as_fields(defs, &bundle)?;
            check_args_count(exprs.len(), &fields).with_context(|| format!("'{path}'"))?;
            let values = exprs
                .iter()
                .zip(&fields)
                .map(|(e, f)| {
                    expr::value_from_expr(e, &f.ty, &bundle)
                        .with_context(|| format!("argument {}", f.ident.as_deref().unwrap_or("_")))
                })
                .collect::<Result<Vec<_>>>()?;
            let return_ty = return_ty.cloned();
            match resource.call(cmd, &values).await? {
                None => Ok("()".to_owned()),
                Some(value @ ValueOwned::Result(Err(_))) => {
                    bail!(
                        "'{path}' failed: {}",
                        fmt_value(&value, return_ty.as_ref(), &bundle)
                    )
                }
                Some(value) => Ok(fmt_value(&value, return_ty.as_ref(), &bundle)),
            }
        }
        DynResourceKind::Property { ty, .. } if args.is_none() => {
            let ty = ty.clone();
            let value = resource.read(cmd).await?;
            Ok(fmt_value(&value, Some(&ty), &bundle))
        }
        DynResourceKind::Property { .. } => {
            bail!("'{path}' is a property: read it as `{path}`, write it as `'{path} = VALUE'`")
        }
        _ => bail!("'{path}' is not a method or a property"),
    }
}

/// `ww call`.
fn command() -> clap::Command {
    CallArgs::augment_args(clap::Command::new("call"))
        .about("Call a method, read a property or write one, from the device's introspection data (TOOL-41)")
}

fn run(run: Run<'_>) -> BoxFuture<'_, Result<Ran>> {
    Box::pin(async move {
        let args = CallArgs::from_arg_matches(&run.matches)?;
        println!("{}", call(run.device(), &args.what).await?);
        Ok(Ran::DONE)
    })
}

inventory::submit! {
    CommandDef::new("call", 41, command, run)
        .globals(&[("", GlobalFlags::Device)])
        .connection(|_| Ok(Connection::Device(DeviceUse::FULL)))
        .log_level(|_| LevelFilter::WARN)
}
