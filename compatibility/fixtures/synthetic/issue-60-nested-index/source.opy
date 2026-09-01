globalvar nested

rule "nested indexed assignments":
    @Event global
    nested[0][1] = 2
    nested[1][0] += 3
    nested[0][1][2] = 4
