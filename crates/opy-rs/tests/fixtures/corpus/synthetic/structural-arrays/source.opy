globalvar g
globalvar h
playervar pv

rule "arrays":
    @Event eachPlayer
    g = g + 1
    g = g.concat([5])
    g = g.exclude([5])
    g = g.exclude(h)
    g.remove(null)
    g.append(h)
    g = g
    eventPlayer.pv[0] = eventPlayer.pv[0] * -1
    eventPlayer.pv[1] = eventPlayer.pv[1] + 2
    g[0] = g[0] + 1
    g = [1, 2, 3].slice(0, 2)
    g = [1, 2, 3].slice(1, 5)
    g = "abc".charAt(1)
    g = [1, 2, 3].last()
    g = [x for x in h if x != 1]
    g = [x for x in h]
    g = round(62.8 * 1000)
    g = ["7-4", "8"][0].split("-")
    g = ["7-4", "8"][1]
