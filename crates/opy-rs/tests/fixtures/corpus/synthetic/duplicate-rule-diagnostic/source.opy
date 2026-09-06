globalvar value

rule "invalid directive":
    @Event global
    @Name "wrong"
    value = 1
