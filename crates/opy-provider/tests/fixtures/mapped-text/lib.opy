#!define bump(v) v += 1

globalvar total

rule "lib":
    @Event global
    total = 0
