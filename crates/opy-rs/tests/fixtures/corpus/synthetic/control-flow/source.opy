globalvar index = 0

rule "control flow":
    @Event global
    for index in range(3):
        if index == 0:
            debug(index)
        elif index == 1:
            debug(index)
        else:
            debug(index)

rule "bounded while":
    @Event global
    while index < 3:
        index += 1
        wait()
