#!optimizeForSize

globalvar g
globalvar h
playervar p
playervar q

rule "size literals":
    @Event eachPlayer
    eventPlayer.p = null
    eventPlayer.q = 0
    g = 0
    g = vect(0, 0, 0)
    g = [0, false, true, vect(0, 0, 0), -1, vect(-1.5, 2, -3)]
    g += 0
    g += 1
    g -= 1
    g.append(0)
    h[0] = 0
    h[1] += 1
    eventPlayer.p[1] = 0
    for h in range(0, 3):
        g += 2
    if g > 0 and h >= 1 and g != 0:
        g = 1 if eventPlayer.p else 0
    wait(0)
