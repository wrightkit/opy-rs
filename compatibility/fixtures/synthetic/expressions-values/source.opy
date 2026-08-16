globalvar result
globalvar points = [1, 2, 3]
globalvar location = vect(1, 2, 3)

rule "expressions and values":
    @Event global
    result = len(points) + 2 * 3
    points.append(result)
    print("points: {}".format(points))
    debug(location)
