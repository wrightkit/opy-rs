globalvar value

rule "issue 33 f string":
    @Event global
    value = f"value {value + 1}"
    debug(sorted([1, 2], key=lambda item: item))
