globalvar total

rule "for loop":
    @Event global

    for I in range(0, 10):
        total += 1
