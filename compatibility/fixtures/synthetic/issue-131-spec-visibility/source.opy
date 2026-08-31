rule "SpecVisibility.NEVER residual":
    @Event global
    hudSubheader(getAllPlayers(), "text", HudPosition.TOP, 0, Color.WHITE, HudReeval.VISIBILITY, SpecVisibility.NEVER)
