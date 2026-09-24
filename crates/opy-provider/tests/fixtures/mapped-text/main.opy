#!include "lib.opy"

rule "main":
    @Event global
    total = 1
    bump(total)
    for I in range(2):
        wait(1)
