globalvar counter = 0
globalvar value = 0

rule "embedded postfix is rejected":
    @Event global
    value = counter++
