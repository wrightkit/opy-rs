globalvar nested

rule "four-dimensional indexed assignment":
    @Event global
    nested[0][0][0][0] = 1
