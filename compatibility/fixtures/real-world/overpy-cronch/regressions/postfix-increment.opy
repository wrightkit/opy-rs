globalvar counter = 0

rule "postfix increment regression":
    @Event global
    counter++
