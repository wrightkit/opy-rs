globalvar value

rule "issue 33 lambda negative":
    @Event global
    debug(lambda item: item)
