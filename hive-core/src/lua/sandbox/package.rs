use crate::Result;
use mlua::{Lua, Table, Function};

pub fn init_package<'a>(lua: &'a Lua, internal: Table<'a>) -> Result<Function<'a>> {
  let package_table = lua.create_table()?;
  let loaded_table = lua.create_table()?;
  let searchers_table = lua.create_table()?;
  package_table.raw_set("loaded", loaded_table.clone())?;
  package_table.raw_set("searchers", searchers_table.clone())?;
  internal.raw_set("package", package_table)?;

  let require_fn = lua
    .load(mlua::chunk! {
      return function(mod_name)
        local mod_name_type = type(mod_name)
        if mod_name_type ~= "string" then
          error("expected string, found" .. mod_name_type)
        end
        if $loaded_table[mod_name] then
          return table.unpack($loaded_table[mod_name])
        else
          for i, searcher in ipairs($searchers_table) do
            local loader, data = searcher(mod_name)
            if type(loader) == "function" then
              local result = { loader(mod_name, data) }
              $loaded_table[mod_name] = result
              return table.unpack(result)
            end
          end
        end
        error("module '" .. mod_name .. "' not found")
      end
    })
    .call(())?;
  Ok(require_fn)
}
