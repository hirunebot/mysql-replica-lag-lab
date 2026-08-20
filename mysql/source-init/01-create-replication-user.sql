CREATE USER IF NOT EXISTS 'repl'@'%'
    IDENTIFIED WITH caching_sha2_password BY 'replpass';

GRANT REPLICATION SLAVE ON *.* TO 'repl'@'%';

