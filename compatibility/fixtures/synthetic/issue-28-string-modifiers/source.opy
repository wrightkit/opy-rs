globalvar value

rule "issue 28 string modifiers":
    @Event global
    value = f"formatted {0}"
    value = w"wide"
    value = b"big"
    value = c"case-sensitive"
