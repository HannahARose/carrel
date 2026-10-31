-- Carrel for Neovim: search an archive and show cards by running `carrel --json`.
--
-- Nothing here reimplements Carrel: every command runs the `carrel` program and reads the
-- JSON it prints (docs/guide/commands.md describes it). No other plugin is needed;
-- Telescope is used for results when it's installed.

local M = {}

local defaults = {
  -- The `carrel` program, as a name on PATH, a path, or a list with arguments.
  cmd = { "carrel" },
  -- The archive to use; when nil, `carrel` finds it from the current folder (or, for
  -- `:CarrelShow`, the file's folder), else uses the one chosen with `carrel open`.
  archive = nil,
  -- How results are listed: "auto" (Telescope if installed, else quickfix), "telescope",
  -- "quickfix" or "float".
  ui = "auto",
  -- How `:CarrelShow` opens its window: a command such as "vsplit", "split" or "tabnew".
  split = "vsplit",
  -- Opens a path: an item, a card, a collection's design document or the glossary. Set it
  -- to `vim.ui.open` (Neovim 0.10) to open items such as PDFs in their own application.
  open = function(path)
    vim.cmd.edit(vim.fn.fnameescape(path))
  end,
}

M.config = vim.deepcopy(defaults)

--- Sets options; see `:help carrel-setup`.
function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", vim.deepcopy(defaults), opts or {})
end

local function notify(message, level)
  vim.notify("carrel: " .. message, level or vim.log.levels.INFO)
end

local function decode(text)
  local ok, value = pcall(vim.json.decode, text, { luanil = { object = true, array = true } })
  if ok then
    return value
  end
end

--- The error `carrel --json` printed on standard error, between its progress lines.
local function error_of(lines)
  for i = #lines, 1, -1 do
    local value = decode(lines[i])
    if type(value) == "table" and value.error then
      return value.error
    end
  end
  local text = vim.trim(table.concat(lines, "\n"))
  return text ~= "" and text or "carrel failed"
end

--- Runs `carrel --json ARGS...` in `cwd`, then calls `done(result)` with the decoded JSON,
--- or `done(nil, message)` if it failed.
function M.run(args, cwd, done)
  local cmd = M.config.cmd
  cmd = type(cmd) == "string" and { cmd } or vim.deepcopy(cmd)
  if vim.fn.executable(cmd[1]) ~= 1 then
    done(nil, cmd[1] .. " isn't installed or isn't on PATH; set cmd in require('carrel').setup()")
    return
  end
  if M.config.archive then
    vim.list_extend(cmd, { "--archive", vim.fn.expand(M.config.archive) })
  end
  table.insert(cmd, "--json")
  vim.list_extend(cmd, args)
  local stdout, stderr = {}, {}
  local job = vim.fn.jobstart(cmd, {
    cwd = cwd,
    stdout_buffered = true,
    stderr_buffered = true,
    on_stdout = function(_, data)
      stdout = data
    end,
    on_stderr = function(_, data)
      stderr = data
    end,
    on_exit = function(_, code)
      vim.schedule(function()
        if code ~= 0 then
          done(nil, error_of(stderr))
          return
        end
        local value = decode(table.concat(stdout, "\n"))
        if type(value) ~= "table" then
          done(nil, "carrel printed something that isn't JSON")
        else
          done(value)
        end
      end)
    end,
  })
  if job <= 0 then
    done(nil, "couldn't run " .. cmd[1])
  end
end

local function join(root, relative)
  if relative == nil then
    return nil
  end
  return root .. "/" .. relative
end

--- One result as the pickers list it: `title`, `about` (where and when), `passage` and its
--- `highlights` (byte ranges, from 0), and the paths to open: `path` for the item (or the
--- card, for a card-only item; a collection's design document; the glossary) and `card`.
--- `search` is a pattern to find in the opened file, for glossary facts.
function M.entry(root, hit)
  local found = hit.found
  local entry = { hit = hit, highlights = {} }
  if hit.passage then
    entry.passage = hit.passage.text
    for _, range in ipairs(hit.passage.highlights or {}) do
      table.insert(entry.highlights, { range.start, range["end"] })
    end
  end
  if found.type == "item" then
    entry.title = found.title
    local about = { found.collection }
    if found.date then
      table.insert(about, found.date:sub(1, 10))
    end
    table.insert(about, found.kind)
    if found.checked_out then
      table.insert(about, "checked out")
    end
    if found.sensitivity == "sensitive" or found.sensitivity == "restricted" then
      entry.mark = found.sensitivity
    end
    entry.about = table.concat(about, " · ")
    entry.card = join(root, found.card)
    entry.path = join(root, found.item) or entry.card
  elseif found.type == "collection" then
    entry.title = found.title
    entry.about = "the collection " .. found.path
    entry.path = join(root, found.path .. "/_collection.md")
  else
    entry.title = found.term or ""
    entry.about = "in the glossary"
    entry.path = join(root, "_System/glossary.md")
    entry.search = "\\V" .. vim.fn.escape(entry.title, "\\")
  end
  return entry
end

--- The result on one line, and where its passage starts on that line.
function M.line(entry)
  local head = entry.title
  if entry.mark then
    head = head .. " [" .. entry.mark .. "]"
  end
  head = head .. "  " .. entry.about
  if not entry.passage then
    return head, nil
  end
  head = head .. "  "
  return head .. entry.passage, #head
end

--- Opens `entry`'s item, or its card when `card` is true (and it has one).
function M.open(entry, card)
  local path = card and entry.card or entry.path
  M.config.open(path)
  if entry.search and vim.fn.expand("%:p") == vim.fn.fnamemodify(path, ":p") then
    vim.fn.search(entry.search, "cw")
  end
end

local function notice(results)
  if results.words_only then
    notify(results.words_only)
  elseif results.meaning and results.meaning.state == "behind" then
    notify(
      ("the meaning index is behind by %d; they're found by their words until it catches up"):format(
        results.meaning.items
      )
    )
  end
end

local function title_of(results)
  return ("Carrel: %s (%d of %d)"):format(results.query, #results.results, results.total)
end

local pickers = {}

function pickers.quickfix(entries, results, card)
  local items = {}
  for _, entry in ipairs(entries) do
    local text = M.line(entry)
    table.insert(items, {
      filename = card and entry.card or entry.path,
      lnum = 1,
      text = text,
      pattern = entry.search,
      user_data = entry,
    })
  end
  vim.fn.setqflist({}, " ", { title = title_of(results), items = items })
  vim.cmd("botright copen")
end

local namespace = vim.api.nvim_create_namespace("carrel")

function pickers.float(entries, results, card)
  local buf = vim.api.nvim_create_buf(false, true)
  local lines, width = {}, 20
  for _, entry in ipairs(entries) do
    local head = entry.title .. (entry.mark and (" [" .. entry.mark .. "]") or "")
    table.insert(lines, head .. "  " .. entry.about)
    table.insert(lines, "  " .. (entry.passage or ""))
  end
  for _, line in ipairs(lines) do
    width = math.max(width, vim.fn.strdisplaywidth(line))
  end
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  for i, entry in ipairs(entries) do
    local row = (i - 1) * 2
    vim.api.nvim_buf_add_highlight(buf, namespace, "Title", row, 0, #entry.title)
    vim.api.nvim_buf_add_highlight(buf, namespace, "Comment", row, #entry.title, -1)
    for _, range in ipairs(entry.highlights) do
      vim.api.nvim_buf_add_highlight(buf, namespace, "Search", row + 1, range[1] + 2, range[2] + 2)
    end
  end
  vim.bo[buf].modifiable = false
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].filetype = "carrel-results"
  width = math.min(width + 2, math.floor(vim.o.columns * 0.9))
  local height = math.min(#lines, math.floor(vim.o.lines * 0.7))
  local win = vim.api.nvim_open_win(buf, true, {
    relative = "editor",
    width = width,
    height = math.max(height, 1),
    row = math.floor((vim.o.lines - height) / 2) - 1,
    col = math.floor((vim.o.columns - width) / 2),
    style = "minimal",
    border = "rounded",
    title = " " .. title_of(results) .. " ",
  })
  vim.wo[win].cursorline = true
  local function choose(to_card)
    local entry = entries[math.floor((vim.api.nvim_win_get_cursor(win)[1] - 1) / 2) + 1]
    vim.api.nvim_win_close(win, true)
    if entry then
      M.open(entry, to_card)
    end
  end
  local function close()
    vim.api.nvim_win_close(win, true)
  end
  local map = function(lhs, fn)
    vim.keymap.set("n", lhs, fn, { buffer = buf, nowait = true })
  end
  map("<CR>", function()
    choose(card)
  end)
  map("c", function()
    choose(true)
  end)
  map("q", close)
  map("<Esc>", close)
  return buf, win
end

function pickers.telescope(entries, results, card)
  local finders = require("telescope.finders")
  local telescope_pickers = require("telescope.pickers")
  local conf = require("telescope.config").values
  local actions = require("telescope.actions")
  local action_state = require("telescope.actions.state")
  local picker = telescope_pickers.new({}, {
    prompt_title = title_of(results),
    finder = finders.new_table({
      results = entries,
      entry_maker = function(entry)
        local line, at = M.line(entry)
        return {
          value = entry,
          ordinal = entry.title .. " " .. entry.about .. " " .. (entry.passage or ""),
          -- The card is previewed: it says what the item is, whatever its format.
          path = entry.card or entry.path,
          display = function()
            local marks = { { { 0, #entry.title }, "TelescopeResultsIdentifier" } }
            for _, range in ipairs(at and entry.highlights or {}) do
              table.insert(marks, { { at + range[1], at + range[2] }, "TelescopeMatching" })
            end
            return line, marks
          end,
        }
      end,
    }),
    sorter = conf.generic_sorter({}),
    previewer = conf.file_previewer({}),
    attach_mappings = function(prompt, map)
      local function choose(to_card)
        local selected = action_state.get_selected_entry()
        actions.close(prompt)
        if selected then
          M.open(selected.value, to_card)
        end
      end
      actions.select_default:replace(function()
        choose(card)
      end)
      map({ "i", "n" }, "<C-e>", function()
        choose(true)
      end)
      return true
    end,
  })
  picker:find()
  return picker
end

local function chosen_ui()
  local ui = M.config.ui
  if ui == "auto" then
    return pcall(require, "telescope") and "telescope" or "quickfix"
  end
  return ui
end

--- Searches with `words` (a list, filters among them as `carrel search` takes them) and
--- lists the results. With `card`, choosing a result opens its card rather than the item.
--- Calls `done(results)` when it's finished, if given.
function M.search(words, card, done)
  M.run(vim.list_extend({ "search" }, words), nil, function(results, err)
    if not results then
      notify(err, vim.log.levels.ERROR)
    elseif #results.results == 0 then
      notify(("nothing found for “%s”"):format(results.query))
      notice(results)
    else
      local entries = {}
      for _, hit in ipairs(results.results) do
        table.insert(entries, M.entry(results.root, hit))
      end
      notice(results)
      local ui = chosen_ui()
      local picker = pickers[ui]
      if not picker then
        notify("ui must be auto, telescope, quickfix or float, not " .. tostring(ui), vim.log.levels.ERROR)
      else
        picker(entries, results, card)
      end
    end
    if done then
      done(results, err)
    end
  end)
end

M.pickers = pickers

--- The lines of a card's view, from `carrel show --json`.
function M.card_lines(shown)
  local fields = shown.fields or {}
  local lines = { fields.title or shown.card, "" }
  local function row(label, value)
    if value ~= nil and value ~= "" then
      table.insert(lines, ("  %-13s%s"):format(label, value))
    end
  end
  row("kind", fields.kind)
  row("id", fields.id)
  row("item", shown.item)
  row("card", shown.card)
  row("collection", shown.collection)
  row("sensitivity", fields.sensitivity)
  if fields.tags and #fields.tags > 0 then
    row("tags", table.concat(fields.tags, ", "))
  end
  local dates = {}
  for name, value in pairs(fields.dates or {}) do
    table.insert(dates, name .. " " .. value)
  end
  table.sort(dates)
  row("dates", table.concat(dates, " · "))
  row("location", fields.location)
  row("url", fields.url)
  if fields.checked_out then
    row("checked out", ("to %s since %s"):format(fields.checked_out.to, fields.checked_out.since))
  end
  if fields.summary then
    table.insert(lines, "")
    vim.list_extend(lines, vim.split(vim.trim(fields.summary), "\n"))
  end
  if shown.problems and #shown.problems > 0 then
    vim.list_extend(lines, { "", "Problems:" })
    for _, problem in ipairs(shown.problems) do
      table.insert(lines, ("  %s: %s"):format(problem.field, problem.message))
    end
  end
  if shown.history and #shown.history > 0 then
    vim.list_extend(lines, { "", "History:" })
    for _, change in ipairs(shown.history) do
      table.insert(
        lines,
        ("  %s  %s  %-11s %s"):format(
          change.id:sub(1, 10),
          (change.time or ""):sub(1, 16):gsub("T", " "),
          change.operation or "",
          change.summary
        )
      )
    end
  end
  vim.list_extend(lines, { "", "o: open the item · c: open the card · q: close" })
  return lines
end

--- Shows the card of the item or card at `path` (the current buffer's file when nil) in a
--- split. Calls `done(shown, buf)` when it's finished, if given.
function M.show(path, done)
  path = path and path ~= "" and vim.fn.fnamemodify(vim.fn.expand(path), ":p")
    or vim.api.nvim_buf_get_name(0)
  if path == "" then
    notify("this buffer has no file; give :CarrelShow a path", vim.log.levels.ERROR)
    return
  end
  local dir = vim.fn.isdirectory(path) == 1 and path or vim.fn.fnamemodify(path, ":h")
  M.run({ "show", path }, dir, function(shown, err)
    local buf
    if not shown then
      notify(err, vim.log.levels.ERROR)
    else
      buf = M.card_buffer(shown)
    end
    if done then
      done(shown, buf)
    end
  end)
end

--- Puts `shown` in a read-only buffer and shows it in a split, reusing a window that
--- already shows a card.
function M.card_buffer(shown)
  local name = "carrel://" .. ((shown.fields or {}).id or shown.card)
  local buf = vim.fn.bufnr(name)
  if buf == -1 then
    buf = vim.api.nvim_create_buf(false, true)
    vim.api.nvim_buf_set_name(buf, name)
  end
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, M.card_lines(shown))
  vim.bo[buf].modifiable = false
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "hide"
  vim.bo[buf].swapfile = false
  vim.bo[buf].filetype = "carrel-card"
  vim.api.nvim_buf_clear_namespace(buf, namespace, 0, -1)
  vim.api.nvim_buf_add_highlight(buf, namespace, "Title", 0, 0, -1)
  local win
  for _, w in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    if vim.bo[vim.api.nvim_win_get_buf(w)].filetype == "carrel-card" then
      win = w
    end
  end
  if win then
    vim.api.nvim_set_current_win(win)
  else
    vim.cmd(M.config.split)
  end
  vim.api.nvim_win_set_buf(0, buf)
  local item = shown.item and join(shown.root, shown.item)
  local card = join(shown.root, shown.card)
  local function open_in_other(path)
    if not path then
      notify("this is a card-only item; there's no file to open")
      return
    end
    vim.cmd("wincmd p")
    if vim.api.nvim_get_current_buf() == buf then
      vim.cmd(M.config.split)
    end
    M.config.open(path)
  end
  local map = function(lhs, fn)
    vim.keymap.set("n", lhs, fn, { buffer = buf, nowait = true })
  end
  map("o", function()
    open_in_other(item)
  end)
  map("c", function()
    open_in_other(card)
  end)
  map("q", "<Cmd>close<CR>")
  return buf
end

--- The `carrel index status --json` report, passed to `done(status, err)`.
function M.index_status(done)
  M.run({ "index", "status" }, nil, done)
end

return M
