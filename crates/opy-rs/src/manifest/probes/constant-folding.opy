globalvar g

rule "folding":
    @Event eachPlayer
    g = vectorTowards(vect(1, 0, 0), vect(0, 1, 0))
    g = vectorTowards(vect(0, 0, 0), vect(0, 1, 0))
    g = vectorTowards(Vector.UP, vect(0, 1, 0))
    g = vectorTowards(eventPlayer, eventPlayer)
    g = vect(1, 2, 3).x
    g = vect(eventPlayer, 1, 2).x
    g = Vector.UP.y
    g = eventPlayer.z
    g = log(100)
    g = log(100, Math.E)
    g = log(100, 10)
    g = log(eventPlayer)
    g = log(100, eventPlayer)
    g = hsl(120, 0.5, 0.5)
    g = hsl(0, 1, 0.5, 128)
    g = hsl(eventPlayer, 0.5, 0.5)
    g = lerp(0, 10, 0.5)
    g = lerp(0, 10, eventPlayer)
    g = getSign(-1)
    g = getSign(0)
    g = getSign(eventPlayer)
    g = lineIntersectsSphere(eventPlayer.getPosition(), eventPlayer.getFacingDirection(), vect(0, 0, 0), 1)
