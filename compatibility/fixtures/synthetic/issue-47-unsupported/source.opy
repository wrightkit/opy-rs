globalvar value
rule "issue 47 unsupported nested switch break":
    @Event global
    switch value:
        case 1:
            if value == 1:
                break
