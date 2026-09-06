globalvar value

rule "issue 47 switch order":
    @Event global
    switch value:
        default:
            value = 4
        case 1:
            value = 1
        case 2:
            value = 2
