--- @class fs
local fs = {}

--- @alias OpenMode "r" | "w" | "a" | "r+" | "w+" | "a+"

--- @async
--- @param path string
--- @param mode OpenMode
--- @return File?
--- @return string? error
function fs.open(path, mode) end

--- @param file any
--- @return "file" | "closed file" | nil
function fs.type(file) end

--- @async
--- @return File?
--- @return string? error
function fs.tmpfile() end

--- @async
--- @param path string
--- @return boolean?
--- @return string? error
function fs.mkdir(path) end

--- @async
--- @param path string
--- @param all? boolean
--- @return boolean?
--- @return string? error
function fs.remove(path, all) end

--- @async
--- @param from string
--- @param to string
--- @return boolean?
--- @return string? error
function fs.rename(from, to) end

--- @async
--- @param path string
--- @return { kind: "dir" | "file", size: integer? }
function fs.metadata(path) end

--- @class File
local File = {}

--- @alias ReadMode "a" | "l" | "L" | integer

--- @async
--- @param ... ReadMode
--- @return string? ...
--- @return string? error
function File:read(...) end

--- @async
--- @param ... string
--- @return File?
--- @return string? error
function File:write(...) end

--- @async
--- @param whence "set" | "cur" | "end"
--- @param pos? integer
--- @return integer?
--- @return string? error
function File:seek(whence, pos) end

--- @param mode ReadMode? Defaults to `l`
--- @return fun(): string
function File:lines(mode) end

--- @async
--- @return boolean?
--- @return string? error
function File:flush() end

--- @return ByteStream
function File:into_stream() end

return fs
