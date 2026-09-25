#!optimizeForSize

globalvar g
playervar p

rule "size rewrites":
    @Event eachPlayer
    createBeam(eventPlayer, Beam.GRAPPLE, Vector.UP, Vector.DOWN, Color.RED, EffectReeval.NONE)
    progressBarHud(eventPlayer, 0, "", HudPosition.LEFT, 3, Color.RED, Color.RED, ProgressHudReeval.COLOR, SpecVisibility.DEFAULT)
    progressBarHud(eventPlayer, g, g, HudPosition.LEFT, 3, Color.RED, Color.RED, ProgressHudReeval.COLOR, SpecVisibility.DEFAULT)
    g = "abc".split("")
    hudText(eventPlayer, "", g, "", HudPosition.LEFT, 3, Color.YELLOW, Color.YELLOW, Color.YELLOW, HudReeval.VISIBILITY_AND_STRING, SpecVisibility.DEFAULT)
    waitUntil(directionTowards(Vector.UP, Vector.UP), 3)
