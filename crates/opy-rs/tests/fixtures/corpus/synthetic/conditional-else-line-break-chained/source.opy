globalvar Mode
globalvar Text
globalvar Name
globalvar Items

rule "chained else line break":
    @Event global
    Text = (
        "{} is idle".format(Name) if Mode == 0 else
        "{} is running".format(Name) if Mode == 1 else
        "{} is {}".format(Name, Mode)
    )
    Items = [
        "a" if Mode == 0 else
        "b",
    ]
