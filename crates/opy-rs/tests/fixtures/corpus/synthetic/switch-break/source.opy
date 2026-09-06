globalvar value

rule "issue 33 switch break":
    @Event global
    switch value:
        case 1:
            value = 2
        case 2:
            value = 3
            break
        default:
            value = 4
    while value < 10:
        switch value:
            case 3:
                break
            default:
                break
        break
