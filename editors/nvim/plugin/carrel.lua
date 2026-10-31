-- The commands; the work is in lua/carrel/init.lua.
if vim.g.loaded_carrel then
  return
end
vim.g.loaded_carrel = true

vim.api.nvim_create_user_command("CarrelSearch", function(opts)
  require("carrel").search(opts.fargs, opts.bang)
end, {
  nargs = "+",
  bang = true,
  desc = "Search the Carrel archive; with !, choosing a result opens its card",
})

vim.api.nvim_create_user_command("CarrelShow", function(opts)
  require("carrel").show(opts.args)
end, {
  nargs = "?",
  complete = "file",
  desc = "Show the card of the item or card in this buffer, or of PATH",
})

vim.api.nvim_create_user_command("CarrelIndex", function()
  require("carrel").index_status(function(status, err)
    if not status then
      vim.notify("carrel: " .. err, vim.log.levels.ERROR)
    elseif not status.model then
      vim.notify("carrel: no embedding model yet, so search uses words only")
    elseif not status.exists then
      vim.notify(("carrel: no meaning index for %s yet; run carrel index rebuild"):format(status.model))
    else
      vim.notify(
        ("carrel: %s: %d of %d items embedded, %d behind"):format(
          status.model,
          status.embedded,
          status.items,
          status.behind
        )
      )
    end
  end)
end, { nargs = 0, desc = "Show how much of the Carrel archive the meaning index covers" })
