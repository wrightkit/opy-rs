playervar p

rule "string replace keeps reevaluating replacements":
    @Event eachPlayer
    eventPlayer.p = "text".replace("x", [1, 2])
    eventPlayer.p = eventPlayer.p.replace("x", "y")
    eventPlayer.p = "text".replace("x", eventPlayer.p)
    eventPlayer.p = "text".replace("x", eventPlayer.p.substring(0, 1))
    eventPlayer.p = "text".replace("x", eventPlayer)
    eventPlayer.p = "text".replace("x", Vector.UP)
