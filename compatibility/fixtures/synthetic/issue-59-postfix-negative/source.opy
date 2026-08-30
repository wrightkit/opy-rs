globalvar counter = 0
globalvar value = 0

rule "prefix is rejected":
    @Event global
    ++counter

rule "postfix stays statement-only":
    @Event global
    value = counter++
