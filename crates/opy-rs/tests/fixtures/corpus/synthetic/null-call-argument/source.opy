# `null` is the only null literal; upstream OverPy rejects `None`.
rule "x":
    @Event eachPlayer
    damage(eventPlayer, null, 10)
