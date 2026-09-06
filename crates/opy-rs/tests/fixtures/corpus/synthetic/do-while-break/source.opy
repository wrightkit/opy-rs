globalvar value

rule "issue 47 do while direct":
    @Event global
    do:
        value += 1
        break
        value += 10
    while value < 2

rule "issue 47 do while conditional":
    @Event global
    do:
        value += 1
        if value == 1:
            break
        value += 10
    while value < 2

rule "issue 47 do while nested":
    @Event global
    do:
        do:
            value += 1
            break
        while value < 2
        value += 10
    while value < 2

rule "issue 47 do while structured tail":
    @Event global
    do:
        value += 1
        break
        if value == 1:
            value += 2
        value += 10
    while value < 2
