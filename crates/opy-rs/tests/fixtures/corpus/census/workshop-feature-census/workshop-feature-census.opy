globalvar counter = 0

rule "workshop feature census":
    @Event global
    if counter == 0:
        counter += 1
        disableInspector()
        wait()
