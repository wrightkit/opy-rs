globalvar result
globalvar left
globalvar right
rule "review 329 helpers":
    @Event global
    result = strVisualLength("Wi")
    result = spacesForLength(142)
    result = spacesForString("W")
    result = arrayToString([1, 2, 3], 1)
    result = timeToString(61)
    tabular([left, right], [1, 2, 3, 4], true)
    createCasedProgressBarIwt(2, getAllPlayers(), "abc", vect(0, 0, 0), 1)
