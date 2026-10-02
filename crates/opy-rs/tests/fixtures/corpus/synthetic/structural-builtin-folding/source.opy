globalvar g
globalvar h

rule "constant folding":
    @Event global
    g = sin(3)
    g = cos(0.5)
    g = sinDeg(3)
    g = cosDeg(60)
    g = sqrt(3)
    g = sqrt(-4)
    g = min(4, 2)
    g = max(4, 2)
    g = distance(vect(1, 2, 3), vect(4, 6, 3))
    g = distance(vect(0, 0, 0), h)
    g = distance(h, vect(0, 0, 0))
    g = dotProduct(vect(1, 2, 3), vect(4, 5, 6))
    g = magnitude(vect(1, 1, 0))
    g = directionTowards(vect(1, 1, 0), vect(4, 1, 0))
    g = directionTowards(vect(1, 1, 0), vect(1, 1, 0))
    g = sqrt(dotProduct(h, h))
    g = getOppositeTeam(Team.1)
    g = getOppositeTeam(Team.2)
    g = getOppositeTeam(Team.ALL)
    g = strContains("abc", "d")
    g = "abcdef".substring(1, 3)
    g = "banana".replace("an", "AN")
    g = [1, 2, 3].index(3)
    g = [1, 2, 3].index(5)
    g = [1, h, 3].index(3)
    g = [].index(1)
    g = [1, 2, 3].slice(5, 2)
    g = [1, 2, 3].slice(1, 5)
    g = 1 / 3
    g = 0.12345678901234567
    g = -2 / 3
    g = vectorTowards(vect(1, 0, 0), vect(0, 1, 0))
    g = vectorTowards(vect(0, 0, 0), vect(0, 1, 0))
    g = vectorTowards(Vector.UP, vect(0, 1, 0))
    g = vectorTowards(h, vect(0, 0, 0))
    g = vect(1, 2, 3).x
    g = vect(h, 1, 2).x
    g = Vector.UP.y
    g = vect(1, 2, 3).z
    g = log(100)
    g = log(100, Math.E)
    g = log(100, 10)
    g = log(h)
    g = log(100, h)
    g = hsl(120, 0.5, 0.5)
    g = hsl(0, 1, 0.5, 128)
    g = hsl(h, 0.5, 0.5)
    g = lerp(0, 10, 0.5)
    g = lerp(0, 10, h)
    g = getSign(-1)
    g = getSign(0)
    g = getSign(h)
    g = lineIntersectsSphere(h, h, vect(0, 0, 0), 1)
