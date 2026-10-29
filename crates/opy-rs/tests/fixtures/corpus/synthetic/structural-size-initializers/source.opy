#!optimizeForSize

globalvar g = ""
globalvar v = vect(0, 0, 0)
globalvar n
playervar ps = ""
playervar pv = vect(0, 0, 0)

rule "rule bodies keep size rewrites":
    @Event global
    n = 0
    g = ""
    v = vect(0, 0, 0)
