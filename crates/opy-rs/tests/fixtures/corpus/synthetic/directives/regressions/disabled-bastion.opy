globalvar value

rule "enabled":
    @Event global
    value = 1

rule "disabled":
    @Event global
    @Disabled
    value = 2

rule "disabled delimiter":
    @Disabled
    @Delimiter
