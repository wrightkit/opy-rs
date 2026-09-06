globalvar value

rule "issue 47 switch structured target":
    @Event global
    switch value:
        case 1:
            if value == 1:
                value = 2
            while value < 0:
                value += 1
        case 2:
            value = 3
        default:
            value = 4
