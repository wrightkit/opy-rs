playervar target = +eventPlayer
globalvar initial = +1
globalvar result
rule "unary plus":
    @Event eachPlayer
    result = +1
    result = +result
    result = +eventPlayer
    result = +vect(1, 2, 3)
    result = +(result + 2) * 3
    result = 2 ** +3
    result = +result ** 2
    result = +-result
    result = +(+result)
    result = 1 + +result
    result = ++1
    result = +++1
    result = 1++result
    result = 1++result * 2
    result = +true
    result = +"x"
    result++
