globalvar code
globalvar n

rule "case and default":
    @Event global
    switch code:
        case "":
            n = 1
        default:
            n = 2
