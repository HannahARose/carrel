-- Tests for the Neovim plugin, run by editors/test/run.sh as
--   nvim --headless --clean -l editors/nvim/tests/run.lua
-- against the fake `carrel` in editors/test/bin, which records its arguments in
-- $CARREL_FAKE_LOG and prints the fixture JSON in editors/test/fixtures.

local here = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h")
local plugin = vim.fn.fnamemodify(here, ":h")
local tests_root = vim.fn.fnamemodify(plugin, ":h") .. "/test"
local archive = vim.loop.fs_realpath(tests_root .. "/archive")
local log = assert(os.getenv("CARREL_FAKE_LOG"), "run through editors/test/run.sh")
vim.opt.runtimepath:prepend(plugin)
vim.cmd("runtime plugin/carrel.lua")

local carrel = require("carrel")
local notes = {}
vim.notify = function(message, level)
  table.insert(notes, { message = message, level = level or vim.log.levels.INFO })
end

local function reset()
  carrel.setup({ cmd = tests_root .. "/bin/carrel", ui = "quickfix" })
  notes = {}
  vim.fn.writefile({}, log)
  vim.cmd("silent! %bwipeout!")
  vim.cmd("silent! only")
  vim.fn.setqflist({}, "r")
  vim.cmd("cclose")
end

--- Runs `start(done)` and waits until it calls `done`.
local function await(start)
  local finished, values = false, nil
  start(function(...)
    values = { ... }
    finished = true
  end)
  assert(vim.wait(5000, function()
    return finished
  end, 10), "timed out")
  return unpack(values)
end

--- Waits for a command that runs carrel and has no callback, until `ready()`.
local function wait_for(ready)
  assert(vim.wait(5000, ready, 10), "timed out")
end

local function calls()
  return vim.fn.readfile(log)
end

local function eq(want, got, what)
  if not vim.deep_equal(want, got) then
    error(("%s: wanted %s, got %s"):format(what or "values", vim.inspect(want), vim.inspect(got)), 2)
  end
end

local function has_note(pattern, level)
  for _, note in ipairs(notes) do
    if note.message:find(pattern) and (level == nil or note.level == level) then
      return true
    end
  end
  return false
end

local tests = {}

tests["entries say what each result is and where to open it"] = function()
  local results = await(function(done)
    carrel.run({ "search", "furniture" }, nil, done)
  end)
  eq(archive, vim.loop.fs_realpath(results.root), "root")
  local bill = carrel.entry(results.root, results.results[1])
  eq("Furniture shop bill", bill.title)
  eq("sensitive", bill.mark)
  eq("Holding · 2025-03-02 · file", bill.about)
  eq(results.root .. "/Holding/furniture-bill.txt", bill.path)
  eq(results.root .. "/Holding/furniture-bill.txt.md", bill.card)
  eq("invoice", bill.passage:sub(bill.highlights[1][1] + 1, bill.highlights[1][2]), "highlight after é and —")
  eq("2057-4038", bill.passage:sub(bill.highlights[2][1] + 1, bill.highlights[2][2]))

  local rfc = carrel.entry(results.root, results.results[2])
  eq(results.root .. "/Reading/RFC 9562 UUIDs.md", rfc.path, "a card-only item opens its card")
  eq("Reading · 2024-05-01 · url · checked out", rfc.about)
  eq(nil, rfc.passage)
  eq(nil, rfc.mark, "public items aren't marked")

  local collection = carrel.entry(results.root, results.results[3])
  eq(results.root .. "/Holding/_collection.md", collection.path)
  eq(nil, collection.card)
  local glossary = carrel.entry(results.root, results.results[4])
  eq("Cowley Road", glossary.title)
  eq(results.root .. "/_System/glossary.md", glossary.path)

  local line, at = carrel.line(bill)
  eq("Furniture shop bill [sensitive]  Holding · 2025-03-02 · file  " .. bill.passage, line)
  eq("invoice", line:sub(at + bill.highlights[1][1] + 1, at + bill.highlights[1][2]))
end

tests[":CarrelSearch passes the query and filters straight through"] = function()
  vim.cmd("CarrelSearch tag:receipts after:2025-01 furniture bill")
  wait_for(function()
    return #vim.fn.getqflist() > 0
  end)
  eq({ "[--json][search][tag:receipts][after:2025-01][furniture][bill]" }, calls())
end

tests[":CarrelSearch lists results in quickfix and opens the item"] = function()
  vim.cmd("CarrelSearch furniture")
  wait_for(function()
    return #vim.fn.getqflist() > 0
  end)
  local list = vim.fn.getqflist({ title = 1, items = 1 })
  eq("Carrel: furniture bill (4 of 5)", list.title)
  eq(4, #list.items)
  assert(list.items[1].text:find("please pay invoice", 1, true), list.items[1].text)
  eq("quickfix", vim.bo.buftype, "the quickfix window is open")
  assert(has_note("behind by 2"), "says the meaning index is behind")
  vim.cmd("cfirst")
  eq(archive .. "/Holding/furniture-bill.txt", vim.loop.fs_realpath(vim.api.nvim_buf_get_name(0)))
end

tests[":CarrelSearch! opens cards"] = function()
  vim.cmd("CarrelSearch! furniture")
  wait_for(function()
    return #vim.fn.getqflist() > 0
  end)
  vim.cmd("cfirst")
  eq(archive .. "/Holding/furniture-bill.txt.md", vim.loop.fs_realpath(vim.api.nvim_buf_get_name(0)))
end

tests["the float lists results, and Enter opens one, c its card"] = function()
  carrel.setup({ cmd = tests_root .. "/bin/carrel", ui = "float" })
  vim.cmd("CarrelSearch furniture")
  wait_for(function()
    return vim.bo.filetype == "carrel-results"
  end)
  local lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
  eq("Furniture shop bill [sensitive]  Holding · 2025-03-02 · file", lines[1])
  eq("  Café chair — please pay invoice 2057-4038 by Friday.", lines[2])
  eq(8, #lines)
  vim.api.nvim_win_set_cursor(0, { 2, 0 })
  vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes("<CR>", true, false, true), "x", false)
  eq(archive .. "/Holding/furniture-bill.txt", vim.loop.fs_realpath(vim.api.nvim_buf_get_name(0)))

  vim.cmd("CarrelSearch furniture")
  wait_for(function()
    return vim.bo.filetype == "carrel-results"
  end)
  vim.api.nvim_win_set_cursor(0, { 7, 0 })
  vim.api.nvim_feedkeys("c", "x", false)
  eq(archive .. "/_System/glossary.md", vim.loop.fs_realpath(vim.api.nvim_buf_get_name(0)), "glossary facts have no card")
  eq("## Cowley Road", vim.api.nvim_get_current_line(), "the cursor is on the term")
end

tests["Telescope is used when it's installed"] = function()
  -- A stand-in for the parts of Telescope the plugin uses.
  local made, mappings, closed = nil, {}, false
  local default = { replace = function(_, fn)
    mappings.default = fn
  end }
  local selected
  package.loaded["telescope"] = {}
  package.loaded["telescope.finders"] = { new_table = function(spec)
    return spec
  end }
  package.loaded["telescope.config"] = { values = {
    generic_sorter = function()
      return "sorter"
    end,
    file_previewer = function()
      return "previewer"
    end,
  } }
  package.loaded["telescope.actions"] = { select_default = default, close = function()
    closed = true
  end }
  package.loaded["telescope.actions.state"] = { get_selected_entry = function()
    return selected
  end }
  package.loaded["telescope.pickers"] = { new = function(_, spec)
    made = spec
    spec.attach_mappings(1, function(_, lhs, fn)
      mappings[lhs] = fn
    end)
    return { find = function() end }
  end }
  local ok, err = pcall(function()
    carrel.setup({ cmd = tests_root .. "/bin/carrel" })
    vim.cmd("CarrelSearch furniture")
    wait_for(function()
      return made ~= nil
    end)
    eq("Carrel: furniture bill (4 of 5)", made.prompt_title)
    local entry = made.finder.entry_maker(made.finder.results[1])
    eq(archive .. "/Holding/furniture-bill.txt.md", vim.loop.fs_realpath(entry.path), "the card is previewed")
    local line, marks = entry.display()
    eq("invoice", line:sub(marks[2][1][1] + 1, marks[2][1][2]))
    selected = entry
    mappings.default()
    assert(closed, "the picker closes")
    eq(archive .. "/Holding/furniture-bill.txt", vim.loop.fs_realpath(vim.api.nvim_buf_get_name(0)))
    mappings["<C-e>"]()
    eq(archive .. "/Holding/furniture-bill.txt.md", vim.loop.fs_realpath(vim.api.nvim_buf_get_name(0)))
  end)
  for _, name in ipairs({ "", ".finders", ".config", ".actions", ".actions.state", ".pickers" }) do
    package.loaded["telescope" .. name] = nil
  end
  assert(ok, err)
end

tests["nothing found says so and opens nothing"] = function()
  await(function(done)
    carrel.search({ "nothing" }, false, done)
  end)
  assert(has_note("nothing found for “nothing”"), vim.inspect(notes))
  assert(has_note("searched by words only"), "says why it used words only")
  eq(0, #vim.fn.getqflist())
end

tests["errors from carrel are shown"] = function()
  local results = await(function(done)
    carrel.search({ "broken:" }, false, done)
  end)
  eq(nil, results)
  assert(has_note("the filter broken: isn't one search knows", vim.log.levels.ERROR), vim.inspect(notes))
  carrel.setup({ cmd = "carrel-that-is-not-installed" })
  local _, err = await(function(done)
    carrel.run({ "search", "x" }, nil, done)
  end)
  assert(err:find("isn't installed"), err)
end

tests[":CarrelShow shows the current file's card"] = function()
  vim.cmd.edit(vim.fn.fnameescape(archive .. "/Holding/furniture-bill.txt"))
  local item_win = vim.api.nvim_get_current_win()
  vim.cmd("CarrelShow")
  wait_for(function()
    return vim.bo.filetype == "carrel-card"
  end)
  eq({ "[--json][show][" .. archive .. "/Holding/furniture-bill.txt]" }, calls())
  local lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
  eq("Furniture shop bill", lines[1])
  local text = table.concat(lines, "\n")
  for _, want in ipairs({
    "  kind         file",
    "  card         Holding/furniture-bill.txt.md",
    "  sensitivity  sensitive",
    "  tags         receipts",
    "  dates        added 2026-10-26 · document 2025-03-02",
    "The bill for a café chair from the furniture shop.",
    "  tags: receipts isn't in the collection's vocabulary",
    "  4f3c2a1b0e  2026-10-26 10:15  file        Filed Furniture shop bill in Holding",
  }) do
    assert(text:find(want, 1, true), ("missing %q in\n%s"):format(want, text))
  end
  eq(false, vim.bo.modifiable)
  eq(2, #vim.api.nvim_tabpage_list_wins(0), "in a split")
  local card_win = vim.api.nvim_get_current_win()

  vim.api.nvim_feedkeys("c", "x", false)
  eq(item_win, vim.api.nvim_get_current_win(), "the card opens beside its view")
  eq(archive .. "/Holding/furniture-bill.txt.md", vim.loop.fs_realpath(vim.api.nvim_buf_get_name(0)))

  vim.fn.writefile({}, log)
  vim.cmd("CarrelShow")
  wait_for(function()
    return #calls() > 0 and vim.api.nvim_get_current_win() == card_win
  end)
  eq(2, #vim.api.nvim_tabpage_list_wins(0), "the card's window is reused")
  vim.api.nvim_feedkeys("o", "x", false)
  eq(archive .. "/Holding/furniture-bill.txt", vim.loop.fs_realpath(vim.api.nvim_buf_get_name(0)))
end

tests[":CarrelShow on a file outside the archive says so"] = function()
  vim.cmd.edit(vim.fn.fnameescape(tests_root .. "/README.md"))
  local shown = await(function(done)
    carrel.show(nil, done)
  end)
  eq(nil, shown)
  assert(has_note("isn't an item or card in this archive", vim.log.levels.ERROR), vim.inspect(notes))
end

tests["the archive option is passed to carrel"] = function()
  carrel.setup({ cmd = { tests_root .. "/bin/carrel" }, archive = archive })
  local status = await(function(done)
    carrel.index_status(done)
  end)
  eq({ "[--archive][" .. archive .. "][--json][index][status]" }, calls())
  eq(2, status.behind)
end

local names = vim.tbl_keys(tests)
table.sort(names)
local failed = 0
for _, name in ipairs(names) do
  reset()
  local ok, err = pcall(tests[name])
  if ok then
    io.stdout:write("ok   " .. name .. "\n")
  else
    failed = failed + 1
    io.stdout:write("FAIL " .. name .. "\n     " .. tostring(err):gsub("\n", "\n     ") .. "\n")
  end
end
io.stdout:write(("%d tests, %d failed\n"):format(#names, failed))
os.exit(failed == 0 and 0 or 1)
