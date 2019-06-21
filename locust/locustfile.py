from locust import HttpLocust, TaskSet

def login(l):
    #l.client.post("/login.html", {"username":"ellen_key", "password":"education"})
    l.client.get("/login.html")

def logout(l):
    #l.client.post("/logout.html", {"username":"ellen_key", "password":"education"})
    l.client.get("/logout.html")

def index(l):
    l.client.get("/")

def profile(l):
    l.client.get("/profile.html")

class UserBehavior(TaskSet):
    tasks = {index: 2, profile: 1}

    def on_start(self):
        login(self)

    def on_stop(self):
        logout(self)

class WebsiteUser(HttpLocust):
    task_set = UserBehavior
    min_wait = 500
    max_wait = 2000
    
