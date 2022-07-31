--- @class fs
local fs = {}

--- @alias OpenMode "r" | "w" | "a" | "r+" | "w+" | "a+"

--- @async
--- @param path string
--- @param mode OpenMode?
--- @return File
function fs.open(path, mode) end

--- @param file any
--- @return "file" | "closed file" | nil
function fs.type(file) end

--- @async
--- @return File
function fs.tmpfile() end

--- @async
--- @param path string
function fs.mkdir(path) end

--- @async
--- @param path string
--- @param all? boolean
function fs.remove(path, all) end

--- @async
--- @param from string
--- @param to string
function fs.rename(from, to) end

--- @async
--- @param path string
--- @return { kind: "dir" | "file", size: integer? }
function fs.metadata(path) end

--- @async
--- @param path string
--- @return boolean
function fs.exists(path) end

--- @class File
local File = {}

--- @alias ReadMode "a" | "l" | "L" | integer

--- @async
--- @param ... ReadMode
--- @return string ...
function File:read(...) end

--- @async
--- @param ... string
--- @return File
function File:write(...) end

--- @async
--- @param whence "set" | "cur" | "end"
--- @param pos? integer
--- @return integer
function File:seek(whence, pos) end

--- @param mode ReadMode? Defaults to `l`
--- @return fun(): string
function File:lines(mode) end

--- @async
function File:flush() end

--- @return ByteStream
function File:into_stream() end

return fs
