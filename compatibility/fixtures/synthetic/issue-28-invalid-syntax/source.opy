globalvar value

rule "issue 28 invalid syntax":
    @Event global
    do:
        value = 1
    while
    value = {"key"}
    value = u"invalid modifier"
