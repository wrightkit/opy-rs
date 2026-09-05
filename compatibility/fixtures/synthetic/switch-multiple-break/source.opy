globalvar value

rule "issue 47 switch multiple break":
    @Event global
    switch value:
        case 1:
            value = 1
            break
        case 2:
            value = 2
            break
        case 3:
            value = 3
        default:
            value = 4
