globalvar value
globalvar i

rule "for continue before later breaks":
    @Event global
    for i in range(5):
        switch value:
            case 1:
                value += 1
                continue
            case 2:
                value += 2
                break
            case 3:
                value += 3
                break
        value += 5

rule "while continue before a later break":
    @Event global
    while value < 10:
        switch value:
            case 1:
                value += 1
                continue
            default:
                value += 2
                break
        value += 5

rule "conditional continue before the arm's own break":
    @Event global
    while value < 10:
        switch value:
            case 1:
                if value == 1:
                    continue
                value += 1
                break
            case 2:
                value += 2
        value += 5

rule "dead code after own break":
    @Event global
    for i in range(5):
        switch value:
            case 1:
                if value == 1:
                    continue
                break
                value += 7
            case 2:
                if value == 2:
                    continue
                value += 2
                break
            default:
                value += 3
        value += 5
