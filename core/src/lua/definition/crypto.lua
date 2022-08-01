--- @class crypto
local crypto = {}

--- @class ThreadRng: Rng
crypto.ThreadRng = {}

--- @class Rng
local Rng = {}

--- @return number
function Rng:random() end

--- @param low integer
--- @param high integer
--- @return integer
function Rng:gen_range(low, high) end

return crypto
