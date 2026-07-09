FROM node:20-alpine AS builder
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM node:20-alpine
WORKDIR /app
COPY --from=builder /app/dist ./dist
COPY --from=builder /app/servidor.mjs ./servidor.mjs

ENV PUERTO=8080
EXPOSE 8080
CMD ["node", "servidor.mjs"]
