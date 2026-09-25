globalvar g
globalvar h

rule "folds see the authored literal":
    @Event global
    g = min(false, 3)
    g = min(3, true)
    g = max(false, 3)
    g = max(3, true)
    g = [1, 2].slice(false, 3)
    g = [1, 2].slice(3, true)
    g = [1, 2].index(0)
    g = "s".replace([], "s")
    g = "s".replace("s", [])
    g = "s".replace("s", null)
    hudText(getAllPlayers(), [], 3, 3, HudPosition.LEFT, 3, Color.YELLOW, Color.YELLOW, Color.YELLOW, HudReeval.VISIBILITY_SORT_ORDER_STRING_AND_COLOR, SpecVisibility.DEFAULT)
