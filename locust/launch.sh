#!/bin/bash

for i in {1..10}; do
    locust --slave >> slave.${i}.log 2>&1 &
done

locust --host=http://${@} --master
