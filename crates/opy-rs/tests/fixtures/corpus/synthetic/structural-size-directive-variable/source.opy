#!optimizeForSize
#!replaceEmptyStringByVariable

globalvar arr
globalvar n
globalvar i
globalvar code
playervar parr
playervar pn
playervar pi

rule "values":
    @Event eachPlayer
    n = ""
    arr -= ""
    arr.append("")
    arr.remove("")
    arr[0] -= ""
    arr[0].append("")
    eventPlayer.parr.append("")
    eventPlayer.pn -= ""
    n = min(n, "")
    n = max(n, "")
    for i in range(""):
        pass
    for i in range("", 5):
        pass
    for i in range(1, 5, ""):
        pass
    for eventPlayer.pi in range(""):
        pass
    arr = ["", "x"]
    switch code:
        case "":
            n = 1
        default:
            n = 2
