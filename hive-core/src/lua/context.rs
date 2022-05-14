use mlua::{Function, Table, ToLua};

pub fn context_enter(context: &Table) -> mlua::Result<()> {
  context.raw_get::<_, Function>("enter")?.call(())
}

pub fn context_exit(context: &Table) -> mlua::Result<()> {
  context.raw_get::<_, Function>("exit")?.call(())
}

pub fn context_register<'lua>(context: &Table<'lua>, item: impl ToLua<'lua>) -> mlua::Result<()> {
  context.raw_get::<_, Function>("register")?.call(item)
}
