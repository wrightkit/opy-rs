rule "invalid binder":
    @Event global
    for 1 in range(3):
        pass
