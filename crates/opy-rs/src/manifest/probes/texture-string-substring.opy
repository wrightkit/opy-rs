globalvar value

rule "string substring":
    @Event global
    value = "______________________________________________________________________________________________________________________________〼".substring(126, true)
