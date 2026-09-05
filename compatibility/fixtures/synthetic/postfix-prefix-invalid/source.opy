globalvar counter = 0
globalvar value = 0

rule "prefix is rejected":
    @Event global
    ++counter
