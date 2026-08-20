FROM php:8.4-cli

RUN docker-php-ext-install pdo_mysql pcntl

WORKDIR /app

COPY . /app/php

CMD ["php", "/app/php/bin/demo.php"]
